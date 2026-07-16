use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use hyperion_agent_runtime::{AgentError, AgentRuntime, InvokeOutcome, LifecycleState, TrustTier};
use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_explainability::{
    ControlState, ExplanationId, ExplanationRecord, ExplanationStore, ReasoningStep,
};
use hyperion_intent::{ExecutionTicket, IntentEngine};
use hyperion_knowledge_graph::{EdgeOrigin, KnowledgeGraph, NodeId};

use crate::catalog::{
    best_fit_manifest_with_plugins, judgment_class_for, required_capabilities_for,
};
use crate::types::{
    AllocationRecord, ConflictKind, ConflictRecord, ConflictResolution, Escalation, JudgmentClass,
    SharedPlan, TaskNode, TaskStatus, WriteOutcome,
};

/// docs/12 §5.4: "reallocation is retried up to a bounded limit before
/// escalating" — one retry, per the doc's own worked example ("on second
/// failure — escalate").
const RETRY_LIMIT: u32 = 1;

/// docs/998-roadmap.md's Self-Sustaining pillar: the same "demoted, never removed" threshold
/// `hyperion-model-router`'s own circuit breaker uses (`CIRCUIT_BREAKER_THRESHOLD`), generalized
/// here to agent *instances* rather than model implementations — see
/// [`CoordinationEngine::allocate`]'s own doc comment on where this is applied.
const REPEAT_OFFENDER_THRESHOLD: u32 = 3;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs()
}

/// The real ranking key `CoordinationEngine::allocate`'s own existing-participant selection
/// sorts candidates by (ascending, so `min_by_key` picks the smallest) — docs/998-roadmap.md's
/// Self-Sustaining pillar, generalizing `hyperion-model-router`'s own circuit breaker's
/// "demoted to the bottom, never removed" precedent from model implementations to agent
/// instances. `false < true` in Rust's own `Ord` for `bool`, so any non-repeat-offender always
/// outranks any repeat offender regardless of load; load only breaks ties *within* each group,
/// never across them. A pure, directly unit-tested function (see `tests` below) rather than
/// only exercised indirectly through the full multi-agent allocation pipeline, since naturally
/// racing two same-specialization instances for the same plan needs a real, live-suspended
/// participant already in that plan — expensive real-time setup a focused unit test on the
/// actual decision doesn't need.
fn ranking_key(times_suspended: u32, task_count: usize) -> (bool, usize) {
    let is_repeat_offender = times_suspended >= REPEAT_OFFENDER_THRESHOLD;
    (is_repeat_offender, task_count)
}

#[cfg(test)]
mod ranking_key_tests {
    use super::*;

    #[test]
    fn a_clean_instance_always_outranks_a_repeat_offender_regardless_of_load() {
        let idle_repeat_offender = ranking_key(REPEAT_OFFENDER_THRESHOLD, 0);
        let busy_clean_instance = ranking_key(0, 100);
        assert!(
            busy_clean_instance < idle_repeat_offender,
            "a busy but clean instance must still rank ahead of an idle repeat offender"
        );
    }

    #[test]
    fn load_still_breaks_ties_among_clean_instances() {
        assert!(ranking_key(0, 1) < ranking_key(0, 5));
        assert!(ranking_key(1, 0) < ranking_key(1, 2));
    }

    #[test]
    fn load_still_breaks_ties_among_repeat_offenders_too() {
        assert!(
            ranking_key(REPEAT_OFFENDER_THRESHOLD, 0) < ranking_key(REPEAT_OFFENDER_THRESHOLD, 3),
            "a repeat offender is demoted, never excluded -- it can still be picked over a \
             busier repeat offender"
        );
    }

    #[test]
    fn the_threshold_is_inclusive() {
        assert!(
            !ranking_key(REPEAT_OFFENDER_THRESHOLD - 1, 0).0,
            "one suspension short of the threshold is still a clean instance"
        );
        assert!(
            ranking_key(REPEAT_OFFENDER_THRESHOLD, 0).0,
            "reaching the threshold exactly is already a repeat offender"
        );
    }
}

/// docs/12 §12's "object-affinity" plan partitioning, real: two tasks belong to the same real
/// partition exactly when they're connected — in either direction, transitively — by real
/// `TaskNode::dependencies` edges, the same "unrelated branches" grouping §12's own worked
/// example (Documentation vs. Deployment) describes. The partition label itself is the lowest
/// real `NodeId` in the connected component, so it's stable and deterministic without needing an
/// invented branch-naming scheme this crate has no real source for. A pure, directly unit-tested
/// function (see `tests` below) rather than only exercised indirectly through a live multi-branch
/// plan — this workspace's one built-in HTN template is a single connected chain, so a live test
/// alone could never actually exercise two genuinely unrelated partitions.
pub(crate) fn task_partition_key(nodes: &[TaskNode], task_id: NodeId) -> String {
    let mut visited = HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(task_id);
    visited.insert(task_id);
    while let Some(current) = queue.pop_front() {
        for node in nodes {
            let touches_current = node.task_id == current || node.dependencies.contains(&current);
            if !touches_current {
                continue;
            }
            for neighbor in std::iter::once(node.task_id).chain(node.dependencies.iter().copied()) {
                if visited.insert(neighbor) {
                    queue.push_back(neighbor);
                }
            }
        }
    }
    let min_id = visited.iter().map(|id| id.0).min().unwrap_or(task_id.0);
    format!("partition-{min_id}")
}

#[cfg(test)]
mod task_partition_key_tests {
    use super::*;

    fn node(id: u64, dependencies: Vec<u64>) -> TaskNode {
        TaskNode {
            task_id: hyperion_storage::ObjectId(id),
            description: format!("task-{id}"),
            required_capabilities: Vec::new(),
            required_trust_tier: TrustTier::Community,
            assigned_agent: None,
            status: TaskStatus::Unassigned,
            dependencies: dependencies
                .into_iter()
                .map(hyperion_storage::ObjectId)
                .collect(),
            base_version: 0,
            attempts: 0,
            result: None,
            extra_context: None,
            judgment_class: JudgmentClass::Mechanical,
        }
    }

    #[test]
    fn a_dependency_chain_is_one_real_partition() {
        // 1 -> 2 -> 3 (each depends on the one before it).
        let nodes = vec![node(1, vec![]), node(2, vec![1]), node(3, vec![2])];
        let p1 = task_partition_key(&nodes, hyperion_storage::ObjectId(1));
        let p2 = task_partition_key(&nodes, hyperion_storage::ObjectId(2));
        let p3 = task_partition_key(&nodes, hyperion_storage::ObjectId(3));
        assert_eq!(p1, p2);
        assert_eq!(p2, p3);
    }

    #[test]
    fn two_genuinely_unrelated_branches_get_different_real_partitions() {
        // Documentation (1 -> 2) and Deployment (10 -> 11) never share a dependency edge.
        let nodes = vec![
            node(1, vec![]),
            node(2, vec![1]),
            node(10, vec![]),
            node(11, vec![10]),
        ];
        let documentation = task_partition_key(&nodes, hyperion_storage::ObjectId(2));
        let deployment = task_partition_key(&nodes, hyperion_storage::ObjectId(11));
        assert_ne!(
            documentation, deployment,
            "unrelated branches must never share a partition"
        );
    }

    #[test]
    fn the_partition_label_is_the_same_regardless_of_which_member_task_is_asked() {
        let nodes = vec![node(5, vec![]), node(2, vec![5]), node(9, vec![2])];
        let via_5 = task_partition_key(&nodes, hyperion_storage::ObjectId(5));
        let via_2 = task_partition_key(&nodes, hyperion_storage::ObjectId(2));
        let via_9 = task_partition_key(&nodes, hyperion_storage::ObjectId(9));
        assert_eq!(via_5, via_2);
        assert_eq!(via_2, via_9);
        assert_eq!(
            via_5, "partition-2",
            "the lowest real id in the component wins"
        );
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CoordError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("intent engine error: {0}")]
    Intent(#[from] hyperion_intent::IntentError),
    #[error("agent runtime error: {0}")]
    Agent(#[from] hyperion_agent_runtime::AgentError),
    #[error("explainability error: {0}")]
    Explainability(#[from] hyperion_explainability::ExplainabilityError),
    #[error("no such coordination session")]
    NotFound,
    #[error("no such task in this session")]
    TaskNotFound,
    #[error("no built-in specialization fits the required capabilities")]
    NoFit,
}

/// One [`PreparedDispatch`]'s own real dispatch result, paired back up with it once
/// [`CoordinationSession::allocate`]'s concurrent dispatch phase (`std::thread::scope`) joins.
type DispatchOutcome = (PreparedDispatch, Result<InvokeOutcome, AgentError>);

/// One real capability dispatch a tick decided to make, and everything
/// [`CoordinationSession::apply_dispatch_results`] needs to record its real outcome once
/// [`CoordinationSession::allocate`]'s own concurrent dispatch phase returns it.
struct PreparedDispatch {
    idx: usize,
    task_id: NodeId,
    agent_id: u64,
    capability: String,
    args: serde_json::Value,
    explanation_id: ExplanationId,
}

/// docs/12 — Multi-Agent Coordination. See this crate's doc comment for
/// what's deferred.
pub struct CoordinationSession {
    agent_runtime: Arc<AgentRuntime>,
    /// Where [`Self::allocate`] records each real capability dispatch's own real result -- a new
    /// `"task_result"` node, linked back to the task's own Intent node via a real `"produced"`
    /// edge -- so what a capability actually produced is genuinely recorded, explorable, and
    /// explainable, not thrown away the instant `invoke` returns (a real, previously-shipped bug
    /// this crate's own doc comment now records).
    graph: Arc<KnowledgeGraph>,
    plans: Mutex<HashMap<u64, SharedPlan>>,
    escalations: Mutex<HashMap<u64, Vec<Escalation>>>,
    /// Test/simulation seam: a `(session, task)` pair queued here forces
    /// that task's *next* capability invocation to fail — see this crate's
    /// doc comment on why `hyperion-agent-runtime` has no real Capability
    /// that can fail on its own yet.
    pending_failures: Mutex<HashSet<(u64, NodeId)>>,
    next_session_id: AtomicU64,
    next_conflict_id: AtomicU64,
    /// docs/18's Explanation Record store for this session's own
    /// `allocate`-driven invocations — see [`Self::allocate`] and
    /// [`Self::explanation`]. Owned here, not by `hyperion-agent-runtime`
    /// itself, because that crate can't depend on `hyperion-explainability`
    /// without a real dependency cycle through `hyperion-recovery` — see
    /// this crate's doc comment. `Arc`-shared (not a private, owned value)
    /// so [`Self::new_with_shared_explanations`] can hand this session the
    /// same real store `hyperion-federation`/`hyperion-api-gateway` use —
    /// docs/998-roadmap.md's own named "workspace-wide, shared Explanation
    /// Record store" gap, closed for a caller that wants it.
    explanations: Arc<ExplanationStore>,
    /// docs/998-roadmap.md's own named "`UndoScope::Session`/`UndoScope::Goal`" gap: real, but
    /// optional (the same `Option<Arc<...>>` shape this workspace already uses for every other
    /// optional real backend) — see [`Self::with_recovery`]'s own doc comment for what wiring one
    /// in actually does.
    recovery: Option<Arc<hyperion_recovery::RecoveryService>>,
}

impl CoordinationSession {
    pub fn new(agent_runtime: Arc<AgentRuntime>, graph: Arc<KnowledgeGraph>) -> Self {
        Self::new_with_shared_explanations(agent_runtime, graph, Arc::new(ExplanationStore::new()))
    }

    /// As [`Self::new`], but with a real, caller-supplied [`ExplanationStore`] this session
    /// shares with other real owners (e.g. a `hyperion_federation::FederationHub` or
    /// `hyperion_api_gateway::ApiGateway` in the same process) instead of its own private one —
    /// a single, real, workspace-wide trace instead of several independent ones. Every real
    /// `action_id` this session mints for it comes from the store's own
    /// [`hyperion_explainability::ExplanationStore::next_action_id`], not an owner-local counter
    /// of its own — sharing a store without also sharing that counter would let two different
    /// owners' `action_id`s collide.
    pub fn new_with_shared_explanations(
        agent_runtime: Arc<AgentRuntime>,
        graph: Arc<KnowledgeGraph>,
        explanations: Arc<ExplanationStore>,
    ) -> Self {
        CoordinationSession {
            agent_runtime,
            graph,
            plans: Mutex::new(HashMap::new()),
            escalations: Mutex::new(HashMap::new()),
            pending_failures: Mutex::new(HashSet::new()),
            next_session_id: AtomicU64::new(1),
            next_conflict_id: AtomicU64::new(1),
            explanations,
            recovery: None,
        }
    }

    /// Opts this session into docs/33 §4's `UndoScope::Session`/`UndoScope::Goal` — closes
    /// `hyperion-recovery`'s own previously-named gap: "neither concept has a first-class id
    /// anywhere in this workspace" was false the moment `SharedPlan.session_id`/`root_intent`
    /// existed; what was still missing was a real caller tagging an `ActionRecord` with them.
    /// Once wired, every real task dispatch [`Self::allocate`] completes opens a real,
    /// best-effort recovery point + `ActionRecord` (via
    /// `RecoveryService::record_action_started_with_scope`) around the real `"task_result"` node
    /// it creates, tagged with this session's own real `session_id` and `root_intent` — undoable
    /// later via `RecoveryService::undo(UndoScope::Session(session_id))` or
    /// `UndoScope::Goal(root_intent)`. Honest scope: `"task_result"` is always a *fresh* KG node
    /// (`put_node`'s `existing_id: None`), and this crate's own `hyperion-recovery` dependency
    /// already documents that a recovery point over a not-yet-created object "cannot undo its
    /// creation" — the real, valuable part here is the crash-recovery journaling and the real
    /// session/goal-scoped bookkeeping this now provides, not restoring a deleted result.
    /// Deliberately optional and consuming (`self -> Self`, chained after [`Self::new`]/
    /// [`Self::new_with_shared_explanations`]) rather than a new constructor parameter, matching
    /// this workspace's `Option<Arc<...>>` optional-backend convention without multiplying
    /// constructors for every independent optional dependency.
    pub fn with_recovery(mut self, recovery: Arc<hyperion_recovery::RecoveryService>) -> Self {
        self.recovery = Some(recovery);
        self
    }

    /// docs/18's "queryable Explanation Record" surface for an
    /// allocation's Capability dispatch — see [`Self::allocate`].
    pub fn explanation(&self, id: ExplanationId) -> Option<ExplanationRecord> {
        self.explanations.get(id)
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
    /// into a [`TaskNode`]. Takes a real `hyperion_intent::ExecutionTicket`
    /// (from `IntentEngine::submit`) rather than a bare `NodeId` — that
    /// crate's own doc comment named `submit`/`ExecutionTicket` as never
    /// actually handed off to a real Multi-Agent Coordination; requiring
    /// one here is that hand-off made real, not optional.
    pub fn create_session(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        intent_engine: &IntentEngine,
        ticket: &ExecutionTicket,
    ) -> Result<u64, CoordError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let root = ticket.root;
        let intents = intent_engine.get_graph(monitor, token, root)?;
        // The real utterance a person actually typed -- captured once here so `allocate` can
        // give each task's real capability dispatch genuine context (docs/05's own root Intent
        // record already carries it; this crate just wasn't reading it before).
        let root_utterance = intents
            .iter()
            .find(|i| i.id == root)
            .map(|i| i.raw_utterance.clone())
            .unwrap_or_default();
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
                result: None,
                extra_context: None,
                judgment_class: judgment_class_for(&leaf.predicate),
            });
        }

        let session_id = self.next_session_id.fetch_add(1, Ordering::Relaxed);
        let plan = SharedPlan {
            session_id,
            root_intent: root,
            root_utterance,
            partition_versions: HashMap::new(),
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

    /// A task is ready for [`Self::allocate`]'s own next real dispatch the moment it's
    /// `Unassigned` with every dependency already `Done` -- shared by [`Self::prepare_dispatches`]
    /// (which actually dispatches the ready set) and [`Self::ready_task_descriptions`] (a
    /// read-only peek at the same set, for callers that want to announce what's about to run
    /// *before* the real, potentially slow dispatch happens).
    fn is_ready(plan: &SharedPlan, node: &TaskNode) -> bool {
        node.status == TaskStatus::Unassigned
            && node.dependencies.iter().all(|d| Self::is_done(plan, *d))
    }

    /// The real description (task predicate, e.g. `"market_research"`) of every task that would
    /// become ready on the very next [`Self::allocate`] call -- a read-only peek `hyperion-console`
    /// uses to announce which tasks are about to run *before* blocking on their real (potentially
    /// slow) dispatch, not only after. Requires `WRITE`, not `READ`, because it runs the same
    /// real `propagate_blocking` pass `allocate` itself does first (a failed task's dependents
    /// only ever get marked `Blocked` here or there, never anywhere else) -- this must see and
    /// apply that real state change consistently with `allocate`, not silently skip it.
    pub fn ready_task_descriptions(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        session_id: u64,
    ) -> Result<Vec<String>, CoordError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let mut plans = self.plans.lock().unwrap();
        let plan = plans.get_mut(&session_id).ok_or(CoordError::NotFound)?;
        Self::propagate_blocking(plan);
        Ok(plan
            .nodes
            .iter()
            .filter(|n| Self::is_ready(plan, n))
            .map(|n| n.description.clone())
            .collect())
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
    /// against existing team participants first (least-loaded fit *among
    /// non-repeat-offenders* — docs/998-roadmap.md's Self-Sustaining pillar: an eligible
    /// instance whose real, whole-life `QuotaState.times_suspended` has reached
    /// [`REPEAT_OFFENDER_THRESHOLD`] is demoted to the bottom of this ranking, never excluded
    /// outright, mirroring `hyperion-model-router`'s own circuit breaker's "demoted, never
    /// removed" precedent applied here to agent instances instead of model implementations —
    /// a load-balancing tie among two otherwise-equal repeat offenders is still broken by load,
    /// just underneath every clean instance), else a fresh instance is spawned; every ready
    /// task's assigned capability is then dispatched concurrently (not one at a time -- see
    /// [`Self::prepare_dispatches`]/[`Self::apply_dispatch_results`]), and each real result
    /// advances its own task to `Done` or, on failure, into docs/12 §5.4's containment path.
    pub fn allocate(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        session_id: u64,
    ) -> Result<Vec<AllocationRecord>, CoordError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let dispatches = self.prepare_dispatches(monitor, token, session_id)?;
        if dispatches.is_empty() {
            return Ok(Vec::new());
        }

        // Phase 2 (unlocked, concurrent): the real capability dispatch for every task prepared
        // above -- potentially real, slow network calls to a real cloud model, now genuinely
        // able to overlap in wall-clock time. `self.plans`/`self.graph` are untouched here;
        // `hyperion_agent_runtime::AgentRuntime::invoke`'s own doc comment explains the matching
        // fix one layer down that makes this actually concurrent, not merely parallel-looking
        // (holding its own instance lock across a real dispatch would still serialize everything
        // behind it, however many real OS threads a caller spawns).
        let dispatched: Vec<DispatchOutcome> = std::thread::scope(|scope| {
            let handles: Vec<_> = dispatches
                .into_iter()
                .map(|d| {
                    scope.spawn(move || {
                        let args = d.args.clone();
                        let outcome = self.agent_runtime.invoke(
                            monitor,
                            token,
                            d.agent_id,
                            &d.capability,
                            args,
                        );
                        (d, outcome)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("a dispatch thread never panics"))
                .collect()
        });

        self.apply_dispatch_results(monitor, token, session_id, dispatched)
    }

    /// [`Self::allocate`]'s phase 1: candidate selection/agent assignment for every ready task,
    /// and opening each one's own Explanation Record -- all fast, in-memory bookkeeping.
    /// Deliberately kept sequential (unlike phase 2): each assignment's own least-loaded-instance
    /// calculation must see the *previous* assignment in this same tick already reflected (docs/
    /// 12 §5.1's real load-balancing -- "one research + one writer instance, reused across
    /// tasks" only holds if a tick's second task sees the first task's just-recorded load),
    /// which concurrent assignment would silently break.
    fn prepare_dispatches(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        session_id: u64,
    ) -> Result<Vec<PreparedDispatch>, CoordError> {
        let mut plans = self.plans.lock().unwrap();
        let plan = plans.get_mut(&session_id).ok_or(CoordError::NotFound)?;
        Self::propagate_blocking(plan);

        let ready: Vec<NodeId> = plan
            .nodes
            .iter()
            .filter(|n| Self::is_ready(plan, n))
            .map(|n| n.task_id)
            .collect();

        let mut dispatches = Vec::new();
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
                    (eligible && has_capabilities && trusted_enough)
                        .then_some((id, instance.quota.times_suspended))
                })
                .min_by_key(|&(id, times_suspended)| {
                    let task_count = plan
                        .nodes
                        .iter()
                        .filter(|n| {
                            n.assigned_agent == Some(id)
                                && matches!(n.status, TaskStatus::Claimed | TaskStatus::InProgress)
                        })
                        .count();
                    ranking_key(times_suspended, task_count)
                })
                .map(|(id, _)| id);

            let agent_id = match candidate {
                Some(id) => id,
                None => {
                    let manifest = best_fit_manifest_with_plugins(
                        &required_capabilities,
                        self.agent_runtime
                            .plugin_registry()
                            .map(std::sync::Arc::as_ref),
                    )
                    .ok_or(CoordError::NoFit)?;
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
            // `"goal"` is real, previously-missing context: a real capability dispatch (see
            // `hyperion-agent-runtime::AgentRuntime::dispatch_document_draft`/
            // `dispatch_market_research`) can now build a genuinely useful prompt from what the
            // user actually asked for, not just this task's own bare predicate name.
            // `"extra_context"`, when [`Self::amend_task`] has set one, is the user's own real
            // steering text for a redo -- included only when present, so a task's very first
            // dispatch (never amended) sends exactly the same args shape as before this existed.
            let mut args = serde_json::json!({
                "task": plan.nodes[idx].description,
                "goal": plan.root_utterance,
                "force_fail": force_fail,
            });
            if let Some(extra) = &plan.nodes[idx].extra_context {
                args["extra_context"] = serde_json::Value::String(extra.clone());
            }
            let capability = required_capabilities.first().cloned().unwrap_or_default();

            // docs/18's explain-then-commit, opened before the real
            // dispatch runs — `hyperion-explainability`'s own doc comment
            // names this crate's `allocate` as one of the Phase 3-7 call
            // sites nothing had wired yet.
            let action_id = self.explanations.next_action_id();
            let explanation_id = self.explanations.begin(
                monitor,
                token,
                action_id,
                plan.root_intent.0,
                agent_id,
                &capability,
                vec![],
                now(),
            )?;
            self.explanations.append_step(
                monitor,
                token,
                explanation_id,
                ReasoningStep {
                    step_index: 0,
                    description: format!(
                        "allocated agent {agent_id} to task '{}'",
                        plan.nodes[idx].description
                    ),
                    capability_ref: Some(capability.clone()),
                    inputs_ref: vec![task_id],
                    output_ref: None,
                },
                Vec::new(),
            )?;
            // docs/998-roadmap.md's Backlog "Protect the Human" item: a real, honest signal
            // distinct from the risk-based consent gate — this task is a matter of taste or
            // empathy, and deserves more human involvement regardless of how reversible it is.
            // Advisory only, per CLAUDE.md's User Control principle: never blocks or changes
            // dispatch, just names the reason in the same real Explanation Record.
            if plan.nodes[idx].judgment_class == JudgmentClass::TasteOrEmpathy {
                self.explanations.append_step(
                    monitor,
                    token,
                    explanation_id,
                    ReasoningStep {
                        step_index: 1,
                        description: format!(
                            "'{}' is a matter of taste or empathy, not just reversibility -- \
                             consider more direct human involvement regardless of how easily it \
                             could be undone",
                            plan.nodes[idx].description
                        ),
                        capability_ref: Some(capability.clone()),
                        inputs_ref: vec![task_id],
                        output_ref: None,
                    },
                    Vec::new(),
                )?;
            }
            self.explanations.transition(
                monitor,
                token,
                explanation_id,
                ControlState::Executing,
            )?;

            dispatches.push(PreparedDispatch {
                idx,
                task_id,
                agent_id,
                capability,
                args,
                explanation_id,
            });
        }

        Ok(dispatches)
    }

    /// [`Self::allocate`]'s phase 3: re-acquires `self.plans`' lock (released for the whole of
    /// phase 2) and applies each real dispatch's real outcome back to the shared plan --
    /// unchanged in substance from this function's own pre-concurrency shape, just now running
    /// after every dispatch in the tick has already completed rather than interleaved one at a
    /// time with each dispatch.
    fn apply_dispatch_results(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        session_id: u64,
        dispatched: Vec<DispatchOutcome>,
    ) -> Result<Vec<AllocationRecord>, CoordError> {
        let mut plans = self.plans.lock().unwrap();
        let plan = plans.get_mut(&session_id).ok_or(CoordError::NotFound)?;

        let mut records = Vec::new();
        for (d, invoke_result) in dispatched {
            let invoke_outcome = invoke_result?;
            let (control_state, outcome) = match invoke_outcome {
                InvokeOutcome::Result(value) => {
                    plan.nodes[d.idx].status = TaskStatus::Done;
                    // A real, previously-shipped bug this fixes: this arm used to be
                    // `InvokeOutcome::Result(_)`, discarding a real capability's own real output
                    // the instant it came back -- nothing downstream (not even this crate's own
                    // caller) could ever see what a task actually produced, only that it
                    // succeeded. `TaskNode.result` now carries it directly; the graph write below
                    // is a second, best-effort record (never fails the allocation over a graph
                    // write hiccup) so `hyperion-console`'s `/recall`/`/why`/`/related` can
                    // surface it too, linked back to the task itself.
                    plan.nodes[d.idx].result = Some(value.clone());
                    if let Ok(result_node) =
                        self.graph
                            .put_node(monitor, token, None, "task_result", None, value)
                    {
                        let _ = self.graph.link(
                            monitor,
                            token,
                            d.task_id,
                            "produced",
                            result_node,
                            1.0,
                            EdgeOrigin::Explicit,
                            None,
                            "capability_dispatch",
                            None,
                        );
                        // docs/998-roadmap.md's own named "UndoScope::Session/Goal" gap, closed
                        // for a real caller -- see `Self::with_recovery`'s own doc comment for
                        // the honest scope boundary (a fresh node, so undo can't restore it; the
                        // real value is crash-recovery journaling and session/goal-scoped
                        // bookkeeping). Best-effort, like the graph write above: never fails this
                        // dispatch over a recovery-service hiccup.
                        if let Some(recovery) = &self.recovery {
                            let now_ts = now();
                            if let Ok(point_id) = recovery.recovery_point_create(
                                monitor,
                                token,
                                hyperion_recovery::Trigger::Automatic,
                                &[],
                                now_ts,
                            ) {
                                let action_id = recovery.record_action_started_with_scope(
                                    point_id,
                                    vec![result_node],
                                    None,
                                    Some(session_id),
                                    Some(plan.root_intent),
                                    "task dispatch produced a real result",
                                    now_ts,
                                );
                                let _ = recovery.record_action_committed(action_id);
                            }
                        }
                    }
                    Self::bump_partition_version(plan, d.task_id);
                    (ControlState::Completed, TaskStatus::Done)
                }
                InvokeOutcome::Failed(_) => {
                    self.handle_task_failure(plan, d.idx, session_id);
                    Self::bump_partition_version(plan, d.task_id);
                    (ControlState::RolledBack, plan.nodes[d.idx].status)
                }
                InvokeOutcome::Denied => {
                    // No effect ever occurred — the closest real terminal
                    // state, not "interrupted" (nothing was running to
                    // interrupt).
                    plan.nodes[d.idx].status = TaskStatus::Claimed;
                    (ControlState::RolledBack, TaskStatus::Claimed)
                }
                InvokeOutcome::PendingConsent | InvokeOutcome::QuotaExceeded => {
                    // Leave claimed for a later tick rather than treating a
                    // grant/quota stall as a task failure — genuinely
                    // "paused, waiting on something external," which is
                    // exactly what `Interrupted` means here.
                    plan.nodes[d.idx].status = TaskStatus::Claimed;
                    (ControlState::Interrupted, TaskStatus::Claimed)
                }
            };
            self.explanations
                .transition(monitor, token, d.explanation_id, control_state)?;
            records.push(AllocationRecord {
                task_id: d.task_id,
                agent_instance: d.agent_id,
                outcome,
                explanation_id: d.explanation_id,
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

    /// The real "redo this with more information" verb `hyperion-console`'s own `/redo <task>
    /// <extra instructions>` meta-command uses: resets the named task (matched against its own
    /// real `description`, case-insensitively) back to [`TaskStatus::Unassigned`] with
    /// `extra_context` recorded for its *next* real dispatch (see [`Self::prepare_dispatches`]),
    /// clears its now-stale `result`, and resets `attempts` to `0` -- a deliberate, user-initiated
    /// redo is not an automatic failure retry and must not consume (or be limited by) that
    /// separate, real [`RETRY_LIMIT`] budget. Works regardless of the task's current status
    /// (`Done`, `Failed`, `Blocked`) -- there's no reason redoing should be narrower than what a
    /// real capability failure already recovers from on its own.
    ///
    /// Returns the real, already-`Done` descriptions of every other task that depends on this
    /// one -- callers use this to warn that those tasks already used the *old*, now-superseded
    /// result, since redoing never cascades to them automatically (a real, deliberate choice:
    /// silently invalidating and re-running an entire downstream chain the user didn't ask about
    /// would be a surprising, hard-to-predict side effect, not real user control).
    pub fn amend_task(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        session_id: u64,
        task_name: &str,
        extra_context: String,
    ) -> Result<Vec<String>, CoordError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let mut plans = self.plans.lock().unwrap();
        let plan = plans.get_mut(&session_id).ok_or(CoordError::NotFound)?;
        let idx = plan
            .nodes
            .iter()
            .position(|n| n.description.eq_ignore_ascii_case(task_name))
            .ok_or(CoordError::TaskNotFound)?;
        let task_id = plan.nodes[idx].task_id;

        plan.nodes[idx].status = TaskStatus::Unassigned;
        plan.nodes[idx].assigned_agent = None;
        plan.nodes[idx].result = None;
        plan.nodes[idx].attempts = 0;
        plan.nodes[idx].extra_context = Some(extra_context);
        Self::bump_partition_version(plan, task_id);

        // Redoing a `Failed` task can resolve the real reason a dependent got stuck `Blocked` in
        // the first place -- but `propagate_blocking` (docs/12 §5.4) only ever adds that mark,
        // never removes it once set, so nothing else in this crate would ever re-evaluate it.
        // Giving it back to `Unassigned` is safe either way: if it's still blocked by something
        // else, the very next real `propagate_blocking` pass re-marks it `Blocked` again.
        for node in plan.nodes.iter_mut() {
            if node.status == TaskStatus::Blocked && node.dependencies.contains(&task_id) {
                node.status = TaskStatus::Unassigned;
            }
        }

        let dependents = plan
            .nodes
            .iter()
            .filter(|n| n.status == TaskStatus::Done && n.dependencies.contains(&task_id))
            .map(|n| n.description.clone())
            .collect();
        Ok(dependents)
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
            // No `partition_versions` bump here: a fact's own `(version, value)` pair, above, is
            // already a real, per-key counter -- `writes_to_different_keys_never_conflict`
            // already proves two unrelated facts never contend. Object-affinity partitioning
            // (`Self::bump_partition_version`) exists for *task* status changes, which previously
            // shared one plan-wide counter regardless of which task changed; facts never had that
            // problem to begin with.
            plan.facts.insert(key.to_string(), (new_version, value));
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

    /// docs/12 §12's "object-affinity" plan partitioning: bumps only `task_id`'s own real
    /// partition (see [`task_partition_key`]), never a plan-wide counter every unrelated task
    /// status change also had to share. Called from every real task-status-changing site in this
    /// module (task completion, task failure, [`Self::amend_task`]'s redo) instead of the single
    /// `SharedPlan.version` field this replaces.
    fn bump_partition_version(plan: &mut SharedPlan, task_id: NodeId) {
        let key = task_partition_key(&plan.nodes, task_id);
        *plan.partition_versions.entry(key).or_insert(0) += 1;
    }

    /// The real, queryable half of object-affinity partitioning: how many times `task_id`'s own
    /// real partition has changed (any task status change anywhere in that same connected group),
    /// `0` if never — real, unlike the plan-wide `SharedPlan.version` field this replaces, which
    /// nothing in this crate ever actually read.
    pub fn partition_version(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        session_id: u64,
        task_id: NodeId,
    ) -> Result<u64, CoordError> {
        self.require(monitor, token, RightsMask::READ)?;
        let plans = self.plans.lock().unwrap();
        let plan = plans.get(&session_id).ok_or(CoordError::NotFound)?;
        let key = task_partition_key(&plan.nodes, task_id);
        Ok(plan.partition_versions.get(&key).copied().unwrap_or(0))
    }
}
