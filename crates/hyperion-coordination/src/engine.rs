use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hyperion_agent_runtime::{AgentRuntime, InvokeOutcome, LifecycleState, TrustTier};
use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_intent::IntentEngine;
use hyperion_knowledge_graph::NodeId;

use crate::catalog::{best_fit_manifest, required_capabilities_for};
use crate::types::{
    AllocationRecord, ConflictKind, ConflictRecord, ConflictResolution, Escalation, SharedPlan,
    TaskNode, TaskStatus, WriteOutcome,
};

/// docs/12 §5.4: "reallocation is retried up to a bounded limit before
/// escalating" — one retry, per the doc's own worked example ("on second
/// failure — escalate").
const RETRY_LIMIT: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum CoordError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("intent engine error: {0}")]
    Intent(#[from] hyperion_intent::IntentError),
    #[error("agent runtime error: {0}")]
    Agent(#[from] hyperion_agent_runtime::AgentError),
    #[error("no such coordination session")]
    NotFound,
    #[error("no such task in this session")]
    TaskNotFound,
    #[error("no built-in specialization fits the required capabilities")]
    NoFit,
}

/// docs/12 — Multi-Agent Coordination. See this crate's doc comment for
/// what's deferred.
pub struct CoordinationSession {
    agent_runtime: Arc<AgentRuntime>,
    plans: Mutex<HashMap<u64, SharedPlan>>,
    escalations: Mutex<HashMap<u64, Vec<Escalation>>>,
    /// Test/simulation seam: a `(session, task)` pair queued here forces
    /// that task's *next* capability invocation to fail — see this crate's
    /// doc comment on why `hyperion-agent-runtime` has no real Capability
    /// that can fail on its own yet.
    pending_failures: Mutex<HashSet<(u64, NodeId)>>,
    next_session_id: AtomicU64,
    next_conflict_id: AtomicU64,
}

impl CoordinationSession {
    pub fn new(agent_runtime: Arc<AgentRuntime>) -> Self {
        CoordinationSession {
            agent_runtime,
            plans: Mutex::new(HashMap::new()),
            escalations: Mutex::new(HashMap::new()),
            pending_failures: Mutex::new(HashSet::new()),
            next_session_id: AtomicU64::new(1),
            next_conflict_id: AtomicU64::new(1),
        }
    }

    fn require(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        rights: RightsMask,
    ) -> Result<(), CoordError> {
        monitor
            .check_rights_ok_result(token, rights)
            .map_err(|_| CoordError::Unauthorized)
    }

    /// docs/12 §6's `createSession` + `decompose`, fused: reads the Intent
    /// Graph's leaves via `IntentEngine::get_graph` and each leaf's
    /// prerequisites via `IntentEngine::depends_on_targets`, turning each
    /// into a [`TaskNode`].
    pub fn create_session(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        intent_engine: &IntentEngine,
        root: NodeId,
    ) -> Result<u64, CoordError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let intents = intent_engine.get_graph(monitor, token, root)?;
        let mut nodes = Vec::new();
        for leaf in intents.iter().filter(|i| i.id != root) {
            let dependencies = intent_engine.depends_on_targets(monitor, token, leaf.id)?;
            nodes.push(TaskNode {
                task_id: leaf.id,
                description: leaf.predicate.clone(),
                required_capabilities: required_capabilities_for(&leaf.predicate),
                // The minimum trust required to work this task — Community
                // is the loosest tier, so any registered specialization
                // qualifies unless a caller wants a stricter policy; see
                // docs/12 §5.1's hard eligibility gate.
                required_trust_tier: TrustTier::Community,
                assigned_agent: None,
                status: TaskStatus::Unassigned,
                dependencies,
                base_version: 0,
                attempts: 0,
            });
        }

        let session_id = self.next_session_id.fetch_add(1, Ordering::Relaxed);
        let plan = SharedPlan {
            session_id,
            root_intent: root,
            version: 0,
            nodes,
            participants: Vec::new(),
            conflicts: Vec::new(),
            facts: HashMap::new(),
        };
        self.plans.lock().unwrap().insert(session_id, plan);
        self.escalations
            .lock()
            .unwrap()
            .insert(session_id, Vec::new());
        Ok(session_id)
    }

    fn is_done(plan: &SharedPlan, id: NodeId) -> bool {
        plan.nodes
            .iter()
            .find(|n| n.task_id == id)
            .map(|n| n.status == TaskStatus::Done)
            .unwrap_or(true)
    }

    /// docs/12 §5.4: a failed task's direct dependents are marked
    /// `Blocked` — distinct from simply "not ready yet," since a `Blocked`
    /// task's prerequisite will never complete without intervention.
    fn propagate_blocking(plan: &mut SharedPlan) {
        let failed: HashSet<NodeId> = plan
            .nodes
            .iter()
            .filter(|n| n.status == TaskStatus::Failed)
            .map(|n| n.task_id)
            .collect();
        for node in plan.nodes.iter_mut() {
            if matches!(node.status, TaskStatus::Unassigned | TaskStatus::Claimed)
                && node.dependencies.iter().any(|d| failed.contains(d))
            {
                node.status = TaskStatus::Blocked;
            }
        }
    }

    fn handle_task_failure(&self, plan: &mut SharedPlan, idx: usize, session_id: u64) {
        plan.nodes[idx].attempts += 1;
        if plan.nodes[idx].attempts <= RETRY_LIMIT {
            // docs/12 §11: "requeued through the allocation algorithm
            // exactly as if it were newly created" — the next `allocate`
            // pass will pick a fresh agent instance for it.
            plan.nodes[idx].status = TaskStatus::Unassigned;
            plan.nodes[idx].assigned_agent = None;
        } else {
            plan.nodes[idx].status = TaskStatus::Failed;
            let task_id = plan.nodes[idx].task_id;
            let description = plan.nodes[idx].description.clone();
            self.escalations
                .lock()
                .unwrap()
                .entry(session_id)
                .or_default()
                .push(Escalation {
                    task_id,
                    reason: format!(
                        "'{description}' failed after {} attempt(s) and needs a decision",
                        plan.nodes[idx].attempts
                    ),
                });
        }
    }

    /// docs/12 §5.1's scored allocation, fused with immediate execution —
    /// see this crate's doc comment on why claim and execute are not
    /// separate steps here. Ready tasks (no unmet dependency) are matched
    /// against existing team participants first (least-loaded fit), else a
    /// fresh instance is spawned; the assigned agent's stub capability is
    /// then invoked synchronously and the result advances the task to
    /// `Done` or, on failure, into docs/12 §5.4's containment path.
    pub fn allocate(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        session_id: u64,
    ) -> Result<Vec<AllocationRecord>, CoordError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let mut plans = self.plans.lock().unwrap();
        let plan = plans.get_mut(&session_id).ok_or(CoordError::NotFound)?;
        Self::propagate_blocking(plan);

        let ready: Vec<NodeId> = plan
            .nodes
            .iter()
            .filter(|n| {
                n.status == TaskStatus::Unassigned
                    && n.dependencies.iter().all(|d| Self::is_done(plan, *d))
            })
            .map(|n| n.task_id)
            .collect();

        let mut records = Vec::new();
        for task_id in ready {
            let idx = plan
                .nodes
                .iter()
                .position(|n| n.task_id == task_id)
                .unwrap();
            let required_capabilities = plan.nodes[idx].required_capabilities.clone();
            let required_trust_tier = plan.nodes[idx].required_trust_tier;

            let candidate = plan
                .participants
                .iter()
                .copied()
                .filter_map(|id| {
                    let instance = self.agent_runtime.describe(id)?;
                    let eligible = !matches!(
                        instance.state,
                        LifecycleState::Terminated
                            | LifecycleState::Suspended
                            | LifecycleState::Failed
                    );
                    let has_capabilities = required_capabilities.iter().all(|c| {
                        instance.manifest.baseline_capabilities.contains(c)
                            || instance.manifest.requestable_capabilities.contains(c)
                    });
                    let trusted_enough = instance.manifest.trust_tier <= required_trust_tier;
                    (eligible && has_capabilities && trusted_enough).then_some(id)
                })
                .min_by_key(|&id| {
                    plan.nodes
                        .iter()
                        .filter(|n| {
                            n.assigned_agent == Some(id)
                                && matches!(n.status, TaskStatus::Claimed | TaskStatus::InProgress)
                        })
                        .count()
                });

            let agent_id = match candidate {
                Some(id) => id,
                None => {
                    let manifest =
                        best_fit_manifest(&required_capabilities).ok_or(CoordError::NoFit)?;
                    let id = self
                        .agent_runtime
                        .spawn(monitor, token, manifest, Some(task_id.0))?;
                    plan.participants.push(id);
                    id
                }
            };

            plan.nodes[idx].assigned_agent = Some(agent_id);
            plan.nodes[idx].status = TaskStatus::InProgress;

            let force_fail = self
                .pending_failures
                .lock()
                .unwrap()
                .remove(&(session_id, task_id));
            let args =
                serde_json::json!({"task": plan.nodes[idx].description, "force_fail": force_fail});
            let capability = required_capabilities.first().cloned().unwrap_or_default();

            let outcome =
                match self
                    .agent_runtime
                    .invoke(monitor, token, agent_id, &capability, args)?
                {
                    InvokeOutcome::Result(_) => {
                        plan.nodes[idx].status = TaskStatus::Done;
                        plan.version += 1;
                        TaskStatus::Done
                    }
                    InvokeOutcome::Failed(_) => {
                        self.handle_task_failure(plan, idx, session_id);
                        plan.version += 1;
                        plan.nodes[idx].status
                    }
                    InvokeOutcome::Denied
                    | InvokeOutcome::PendingConsent
                    | InvokeOutcome::QuotaExceeded => {
                        // Leave claimed for a later tick rather than treating a
                        // grant/quota stall as a task failure.
                        plan.nodes[idx].status = TaskStatus::Claimed;
                        TaskStatus::Claimed
                    }
                };
            records.push(AllocationRecord {
                task_id,
                agent_instance: agent_id,
                outcome,
            });
        }

        Self::propagate_blocking(plan);
        Ok(records)
    }

    /// Marks `task_id`'s next allocation attempt within `session_id` to
    /// force-fail — see this crate's doc comment.
    pub fn inject_failure(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        session_id: u64,
        task_id: NodeId,
    ) -> Result<(), CoordError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        self.pending_failures
            .lock()
            .unwrap()
            .insert((session_id, task_id));
        Ok(())
    }

    /// docs/12 §5.2's `proposeWrite`: optimistic concurrency on a named
    /// plan fact. Auto-merge applies trivially to non-colliding keys
    /// (independent facts never conflict by construction); a genuine
    /// same-key collision always raises a `ConcurrentWrite` conflict here,
    /// since real field-level diff auto-merge needs structured diffs this
    /// crate's opaque `serde_json::Value` facts don't carry — see this
    /// crate's doc comment.
    #[allow(clippy::too_many_arguments)]
    pub fn propose_write(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        session_id: u64,
        agent_instance: u64,
        key: &str,
        base_version: u64,
        value: serde_json::Value,
    ) -> Result<WriteOutcome, CoordError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let mut plans = self.plans.lock().unwrap();
        let plan = plans.get_mut(&session_id).ok_or(CoordError::NotFound)?;
        let current_version = plan.facts.get(key).map(|(v, _)| *v).unwrap_or(0);

        if base_version == current_version {
            let new_version = current_version + 1;
            plan.facts.insert(key.to_string(), (new_version, value));
            plan.version += 1;
            return Ok(WriteOutcome::Accepted { new_version });
        }

        let conflict = ConflictRecord {
            conflict_id: self.next_conflict_id.fetch_add(1, Ordering::Relaxed),
            key: key.to_string(),
            claimants: vec![agent_instance],
            kind: ConflictKind::ConcurrentWrite,
            resolution: ConflictResolution::Pending,
        };
        plan.conflicts.push(conflict.clone());
        Ok(WriteOutcome::Conflict(conflict))
    }

    /// docs/12 §5.2's `contradictory_subplan` path: arbitration by the
    /// Intent Graph's own stated priorities, standing in here as a
    /// caller-supplied predicate priority order (docs/12 §8's "legal risk
    /// outranks branding preference by policy") — see this crate's doc
    /// comment on why contradiction *detection* itself is manual.
    pub fn arbitrate_contradiction(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        session_id: u64,
        task_a: NodeId,
        task_b: NodeId,
        priority_order: &[&str],
    ) -> Result<ConflictRecord, CoordError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let mut plans = self.plans.lock().unwrap();
        let plan = plans.get_mut(&session_id).ok_or(CoordError::NotFound)?;
        let predicate_of = |plan: &SharedPlan, id: NodeId| {
            plan.nodes
                .iter()
                .find(|n| n.task_id == id)
                .map(|n| n.description.clone())
        };
        let (pred_a, pred_b) = (
            predicate_of(plan, task_a).ok_or(CoordError::TaskNotFound)?,
            predicate_of(plan, task_b).ok_or(CoordError::TaskNotFound)?,
        );
        let rank = |p: &str| priority_order.iter().position(|candidate| *candidate == p);

        let conflict_id = self.next_conflict_id.fetch_add(1, Ordering::Relaxed);
        let (resolution, loser) = match (rank(&pred_a), rank(&pred_b)) {
            (Some(ra), Some(rb)) if ra != rb => (
                ConflictResolution::CoordinatorResolved,
                if ra < rb { Some(task_b) } else { Some(task_a) },
            ),
            _ => (ConflictResolution::Pending, None),
        };

        if let Some(loser_id) = loser {
            if let Some(node) = plan.nodes.iter_mut().find(|n| n.task_id == loser_id) {
                // docs/12 §8: "reassigns [the loser's] TaskNode back to
                // in_progress with the constraint attached" — this crate
                // requeues it for reallocation rather than modeling a
                // separate "constraint attached" annotation.
                node.status = TaskStatus::Unassigned;
                node.assigned_agent = None;
            }
        }

        let conflict = ConflictRecord {
            conflict_id,
            key: format!("{pred_a}<->{pred_b}"),
            claimants: [task_a, task_b]
                .iter()
                .filter_map(|id| plan.nodes.iter().find(|n| n.task_id == *id)?.assigned_agent)
                .collect(),
            kind: ConflictKind::ContradictorySubplan,
            resolution,
        };
        plan.conflicts.push(conflict.clone());
        if resolution == ConflictResolution::Pending {
            self.escalations.lock().unwrap().entry(session_id).or_default().push(Escalation {
                task_id: task_a,
                reason: format!("'{pred_a}' and '{pred_b}' disagree and neither is prioritized — needs a decision"),
            });
        }
        Ok(conflict)
    }

    /// docs/12 §5.3's weighted progress rollup — uniform weight per task,
    /// see this crate's doc comment on the `object-affinity` scale
    /// optimization this narrow slice doesn't need yet.
    pub fn progress(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        session_id: u64,
    ) -> Result<f32, CoordError> {
        self.require(monitor, token, RightsMask::READ)?;
        let plans = self.plans.lock().unwrap();
        let plan = plans.get(&session_id).ok_or(CoordError::NotFound)?;
        if plan.nodes.is_empty() {
            return Ok(1.0);
        }
        let done = plan
            .nodes
            .iter()
            .filter(|n| n.status == TaskStatus::Done)
            .count() as f32;
        Ok(done / plan.nodes.len() as f32)
    }

    pub fn escalations(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        session_id: u64,
    ) -> Result<Vec<Escalation>, CoordError> {
        self.require(monitor, token, RightsMask::READ)?;
        Ok(self
            .escalations
            .lock()
            .unwrap()
            .get(&session_id)
            .cloned()
            .unwrap_or_default())
    }

    pub fn get_plan(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        session_id: u64,
    ) -> Result<SharedPlan, CoordError> {
        self.require(monitor, token, RightsMask::READ)?;
        self.plans
            .lock()
            .unwrap()
            .get(&session_id)
            .cloned()
            .ok_or(CoordError::NotFound)
    }
}
