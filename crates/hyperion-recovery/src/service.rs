use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hyperion_agent_runtime::AgentRuntime;
use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_knowledge_graph::{GraphError, KnowledgeGraph, NodeId};

use crate::types::{
    ActionId, ActionRecord, ActionStatus, RecoveryError, RecoveryPoint, RecoveryPointId, Snapshot,
    Trigger, UndoReceipt, UndoScope,
};

/// docs/33 — Rollback & Recovery. See this crate's doc comment for the
/// full real/deferred split.
pub struct RecoveryService {
    graph: Arc<KnowledgeGraph>,
    points: Mutex<HashMap<RecoveryPointId, RecoveryPoint>>,
    snapshots: Mutex<HashMap<RecoveryPointId, Snapshot>>,
    actions: Mutex<Vec<ActionRecord>>,
    next_point_id: AtomicU64,
    next_action_id: AtomicU64,
}

impl RecoveryService {
    pub fn new(graph: Arc<KnowledgeGraph>) -> Self {
        RecoveryService {
            graph,
            points: Mutex::new(HashMap::new()),
            snapshots: Mutex::new(HashMap::new()),
            actions: Mutex::new(Vec::new()),
            next_point_id: AtomicU64::new(1),
            next_action_id: AtomicU64::new(1),
        }
    }

    fn require(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        rights: RightsMask,
    ) -> Result<(), RecoveryError> {
        monitor
            .check_rights_ok_result(token, rights)
            .map_err(|_| RecoveryError::Unauthorized)
    }

    /// docs/33 §5's recovery-point creation: capture what each about-to-
    /// be-touched object looked like *before* the triggering action runs.
    /// `None` in the snapshot means the object doesn't exist yet — see
    /// this crate's doc comment on why a freshly created object can't
    /// later be "un-created" (`hyperion-knowledge-graph` has no node
    /// delete).
    pub fn recovery_point_create(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        trigger: Trigger,
        objects_about_to_touch: &[NodeId],
        now: u64,
    ) -> Result<RecoveryPointId, RecoveryError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let mut snapshot = Vec::with_capacity(objects_about_to_touch.len());
        for &id in objects_about_to_touch {
            let existing = match self.graph.get(monitor, token, id) {
                Ok(record) => Some(record),
                Err(GraphError::NotFound) => None,
                Err(e) => return Err(e.into()),
            };
            snapshot.push((id, existing));
        }

        let point_id = self.next_point_id.fetch_add(1, Ordering::Relaxed);
        self.points.lock().unwrap().insert(
            point_id,
            RecoveryPoint {
                id: point_id,
                created_at: now,
                trigger,
                pinned: false,
            },
        );
        self.snapshots.lock().unwrap().insert(point_id, snapshot);
        Ok(point_id)
    }

    /// docs/33 §4's `ActionRecord`, opened when a caller begins an action
    /// whose effects should be undoable/crash-recoverable. Journaled as
    /// `InFlight` until [`Self::record_action_committed`] or
    /// [`Self::record_action_aborted`] closes it — an action never
    /// observed to close is exactly what
    /// [`Self::recover_from_crash`] looks for.
    pub fn record_action_started(
        &self,
        recovery_point_before: RecoveryPointId,
        objects_touched: Vec<NodeId>,
        agent_run_id: Option<u64>,
        note: &str,
        now: u64,
    ) -> ActionId {
        let action_id = self.next_action_id.fetch_add(1, Ordering::Relaxed);
        self.actions.lock().unwrap().push(ActionRecord {
            action_id,
            agent_run_id,
            recovery_point_before,
            objects_touched,
            status: ActionStatus::InFlight,
            created_at: now,
            note: note.to_string(),
        });
        action_id
    }

    fn set_status(&self, action_id: ActionId, status: ActionStatus) -> Result<(), RecoveryError> {
        let mut actions = self.actions.lock().unwrap();
        let record = actions
            .iter_mut()
            .find(|a| a.action_id == action_id)
            .ok_or(RecoveryError::NoSuchAction)?;
        record.status = status;
        Ok(())
    }

    pub fn record_action_committed(&self, action_id: ActionId) -> Result<(), RecoveryError> {
        self.set_status(action_id, ActionStatus::Committed)
    }

    pub fn record_action_aborted(&self, action_id: ActionId) -> Result<(), RecoveryError> {
        self.set_status(action_id, ActionStatus::Aborted)
    }

    fn restore_objects(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        recovery_point_id: RecoveryPointId,
        objects: &[NodeId],
    ) -> Result<(), RecoveryError> {
        let snapshot = self
            .snapshots
            .lock()
            .unwrap()
            .get(&recovery_point_id)
            .cloned()
            .ok_or(RecoveryError::NoSuchRecoveryPoint)?;
        for (node_id, before) in snapshot.iter().filter(|(id, _)| objects.contains(id)) {
            if let Some(record) = before {
                self.graph.put_node(
                    monitor,
                    token,
                    Some(*node_id),
                    record.object_type.clone(),
                    record.embedding.clone(),
                    record.metadata.clone(),
                )?;
            }
        }
        Ok(())
    }

    /// docs/33 §5's `recover_from_crash()` — the Phase 8 exit criterion:
    /// "a corrupted mid-Agent-execution crash recovers cleanly." Every
    /// still-`InFlight` action is rolled back to the state it captured
    /// before it ran, never replayed forward (double-side-effect risk),
    /// then its Agent instance is terminated and re-spawned fresh against
    /// the same manifest and bound Intent — this crate's translation of
    /// "hands control to Agent Runtime to re-plan from clean Intent
    /// state, not resume mid-step," since re-planning itself is [05 —
    /// Intent Engine](../05-intent-engine.md)'s job, invoked whenever the
    /// caller next drives the fresh instance.
    pub fn recover_from_crash(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        agent_runtime: &AgentRuntime,
        now: u64,
    ) -> Result<Vec<ActionId>, RecoveryError> {
        let _ = now;
        let in_flight: Vec<ActionRecord> = self
            .actions
            .lock()
            .unwrap()
            .iter()
            .filter(|a| a.status == ActionStatus::InFlight)
            .cloned()
            .collect();

        let mut recovered = Vec::new();
        for record in &in_flight {
            self.restore_objects(
                monitor,
                token,
                record.recovery_point_before,
                &record.objects_touched,
            )?;

            if let Some(agent_run_id) = record.agent_run_id {
                if let Some(instance) = agent_runtime.describe(agent_run_id) {
                    agent_runtime.terminate(
                        monitor,
                        token,
                        agent_run_id,
                        "crash_recovery_replan",
                    )?;
                    agent_runtime.spawn(
                        monitor,
                        token,
                        instance.manifest,
                        instance.bound_intent,
                    )?;
                }
            }

            self.record_action_aborted(record.action_id)?;
            recovered.push(record.action_id);
        }
        Ok(recovered)
    }

    fn in_scope(record: &ActionRecord, scope: UndoScope) -> bool {
        match scope {
            UndoScope::SingleAction(id) => record.action_id == id,
            UndoScope::AgentRun(run_id) => record.agent_run_id == Some(run_id),
            UndoScope::Global(point_id) => record.recovery_point_before == point_id,
        }
    }

    /// docs/33 §5's `undo(scope)` pseudocode: if every touched object is
    /// untouched by anything outside `scope` since the earliest in-scope
    /// action's recovery point, restore directly; otherwise surface the
    /// conflicting objects and require explicit confirmation rather than
    /// silently overwriting concurrent legitimate work.
    pub fn undo(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        scope: UndoScope,
    ) -> Result<UndoReceipt, RecoveryError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let all_actions = self.actions.lock().unwrap().clone();
        let mut in_scope: Vec<ActionRecord> = all_actions
            .iter()
            .filter(|a| a.status == ActionStatus::Committed && Self::in_scope(a, scope))
            .cloned()
            .collect();
        if in_scope.is_empty() {
            return Ok(UndoReceipt::NothingToUndo);
        }
        in_scope.sort_by_key(|a| std::cmp::Reverse(a.created_at));

        let scope_ids: HashSet<ActionId> = in_scope.iter().map(|a| a.action_id).collect();
        let touched: HashSet<NodeId> = in_scope
            .iter()
            .flat_map(|a| a.objects_touched.iter().copied())
            .collect();
        let earliest = in_scope.iter().map(|a| a.created_at).min().unwrap();

        let conflicts: Vec<NodeId> = all_actions
            .iter()
            .filter(|a| {
                !scope_ids.contains(&a.action_id)
                    && a.status != ActionStatus::Aborted
                    && a.created_at >= earliest
            })
            .flat_map(|a| a.objects_touched.iter().copied())
            .filter(|id| touched.contains(id))
            .collect();

        if !conflicts.is_empty() {
            return Ok(UndoReceipt::NeedsConfirmation {
                conflicting_objects: conflicts,
            });
        }

        let mut undone = Vec::new();
        for record in &in_scope {
            self.restore_objects(
                monitor,
                token,
                record.recovery_point_before,
                &record.objects_touched,
            )?;
            undone.push(record.action_id);
        }

        let mut actions = self.actions.lock().unwrap();
        for a in actions.iter_mut() {
            if scope_ids.contains(&a.action_id) {
                a.status = ActionStatus::Aborted;
            }
        }
        Ok(UndoReceipt::Targeted {
            undone_actions: undone,
        })
    }

    pub fn pin(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: RecoveryPointId,
    ) -> Result<(), RecoveryError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        self.points
            .lock()
            .unwrap()
            .get_mut(&id)
            .ok_or(RecoveryError::NoSuchRecoveryPoint)?
            .pinned = true;
        Ok(())
    }

    pub fn unpin(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: RecoveryPointId,
    ) -> Result<(), RecoveryError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        self.points
            .lock()
            .unwrap()
            .get_mut(&id)
            .ok_or(RecoveryError::NoSuchRecoveryPoint)?
            .pinned = false;
        Ok(())
    }

    pub fn recovery_point(&self, id: RecoveryPointId) -> Option<RecoveryPoint> {
        self.points.lock().unwrap().get(&id).cloned()
    }

    pub fn action_records(&self) -> Vec<ActionRecord> {
        self.actions.lock().unwrap().clone()
    }
}
