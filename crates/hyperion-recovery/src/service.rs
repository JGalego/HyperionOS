use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hyperion_agent_runtime::AgentRuntime;
use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_knowledge_graph::{GraphError, KnowledgeGraph, NodeId};

use crate::types::{
    ActionId, ActionRecord, ActionStatus, RecoveryError, RecoveryPoint, RecoveryPointId,
    RedoReceipt, Snapshot, Trigger, UndoReceipt, UndoScope,
};

/// docs/33 — Rollback & Recovery. See this crate's doc comment for the
/// full real/deferred split.
pub struct RecoveryService {
    graph: Arc<KnowledgeGraph>,
    points: Mutex<HashMap<RecoveryPointId, RecoveryPoint>>,
    snapshots: Mutex<HashMap<RecoveryPointId, Snapshot>>,
    actions: Mutex<Vec<ActionRecord>>,
    /// Captured by [`Self::undo`] at the moment it reverts an action -- the
    /// state those `objects_touched` were actually in right before that
    /// revert, i.e. the action's own real "after" effects. This is what
    /// [`Self::redo`] re-applies; it is keyed by [`ActionId`] rather than
    /// folded into `snapshots` (which is keyed by [`RecoveryPointId`] and
    /// represents "before an action ran," not "after").
    redo_snapshots: Mutex<HashMap<ActionId, Snapshot>>,
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
            redo_snapshots: Mutex::new(HashMap::new()),
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

    /// Reads the current live state of every id in `objects` — `None` for
    /// one that doesn't exist (yet). Shared by [`Self::recovery_point_create`]
    /// (captures "before an action runs") and [`Self::undo`] (captures "the
    /// action's real effects, right before reverting them" for
    /// [`Self::redo`] to later re-apply).
    fn snapshot_objects(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        objects: &[NodeId],
    ) -> Result<Snapshot, RecoveryError> {
        let mut snapshot = Vec::with_capacity(objects.len());
        for &id in objects {
            let existing = match self.graph.get(monitor, token, id) {
                Ok(record) => Some(record),
                Err(GraphError::NotFound) => None,
                Err(e) => return Err(e.into()),
            };
            snapshot.push((id, existing));
        }
        Ok(snapshot)
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

        let snapshot = self.snapshot_objects(monitor, token, objects_about_to_touch)?;

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

    /// Writes every `Some` entry of `snapshot` back to the graph verbatim,
    /// restricted to `objects` — shared by [`Self::restore_objects`] (a
    /// `RecoveryPointId`-keyed "before" snapshot) and [`Self::redo`] (an
    /// `ActionId`-keyed "after" snapshot). A `None` entry is left alone,
    /// same limitation both callers already document: this can't un-create
    /// an object `hyperion-knowledge-graph` has no delete operation for.
    fn apply_snapshot(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        snapshot: &Snapshot,
        objects: &[NodeId],
    ) -> Result<(), RecoveryError> {
        for (node_id, state) in snapshot.iter().filter(|(id, _)| objects.contains(id)) {
            if let Some(record) = state {
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
        self.apply_snapshot(monitor, token, &snapshot, objects)
    }

    /// docs/32/33's `restore_to(recovery_point_id)`: restores every object
    /// this recovery point captured, directly — no `ActionRecord`
    /// involved, unlike [`Self::undo`]'s conflict-checked path. This is
    /// the mechanism [21-style] callers outside this crate (docs/32's
    /// Update System is the motivating case) compose with when they took
    /// their own `recovery_point_create` snapshot and now need to revert
    /// to it wholesale, without having journaled individual
    /// `ActionRecord`s along the way.
    pub fn restore_to(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        recovery_point_id: RecoveryPointId,
    ) -> Result<(), RecoveryError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let all_ids: Vec<NodeId> = self
            .snapshots
            .lock()
            .unwrap()
            .get(&recovery_point_id)
            .ok_or(RecoveryError::NoSuchRecoveryPoint)?
            .iter()
            .map(|(id, _)| *id)
            .collect();
        self.restore_objects(monitor, token, recovery_point_id, &all_ids)
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

        // `Undone` actions are excluded alongside `Aborted`: their effects
        // were already reverted, so a later action touching the same
        // object no longer represents live, conflicting data.
        let conflicts: Vec<NodeId> = all_actions
            .iter()
            .filter(|a| {
                !scope_ids.contains(&a.action_id)
                    && a.status != ActionStatus::Aborted
                    && a.status != ActionStatus::Undone
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

        // Capture each action's real, current effects *before* reverting
        // any of them -- this is what `redo` later re-applies. Done as its
        // own pass, before any restore below runs, so every capture reads
        // genuinely live (pre-revert) state.
        {
            let mut redo_snapshots = self.redo_snapshots.lock().unwrap();
            for record in &in_scope {
                let post_action_state =
                    self.snapshot_objects(monitor, token, &record.objects_touched)?;
                redo_snapshots.insert(record.action_id, post_action_state);
            }
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
                a.status = ActionStatus::Undone;
            }
        }
        Ok(UndoReceipt::Targeted {
            undone_actions: undone,
        })
    }

    /// docs/33's `redo(scope)`: the mirror image of [`Self::undo`],
    /// re-applying an already-undone action's real captured effects rather
    /// than replaying it forward. Same conflict rule: if anything
    /// committed since the undo touched one of the same objects, redoing
    /// would silently clobber that newer, legitimate work, so this
    /// surfaces the conflict and requires explicit confirmation instead,
    /// exactly like `undo` does for concurrent edits.
    pub fn redo(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        scope: UndoScope,
    ) -> Result<RedoReceipt, RecoveryError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let all_actions = self.actions.lock().unwrap().clone();
        let mut in_scope: Vec<ActionRecord> = all_actions
            .iter()
            .filter(|a| a.status == ActionStatus::Undone && Self::in_scope(a, scope))
            .cloned()
            .collect();
        if in_scope.is_empty() {
            return Ok(RedoReceipt::NothingToRedo);
        }
        // Oldest first: the reverse of undo's newest-first unwind, so two
        // redone actions that touched the same object converge back on the
        // same forward order they were originally committed in.
        in_scope.sort_by_key(|a| a.created_at);

        let scope_ids: HashSet<ActionId> = in_scope.iter().map(|a| a.action_id).collect();
        let touched: HashSet<NodeId> = in_scope
            .iter()
            .flat_map(|a| a.objects_touched.iter().copied())
            .collect();
        let earliest = in_scope.iter().map(|a| a.created_at).min().unwrap();

        // Anything genuinely committed against one of these objects since
        // the earliest undo in scope is new, legitimate work done *after*
        // the undo -- redo would silently clobber it, so surface it
        // instead, mirroring undo's own conflict rule.
        let conflicts: Vec<NodeId> = all_actions
            .iter()
            .filter(|a| {
                !scope_ids.contains(&a.action_id)
                    && a.status == ActionStatus::Committed
                    && a.created_at >= earliest
            })
            .flat_map(|a| a.objects_touched.iter().copied())
            .filter(|id| touched.contains(id))
            .collect();

        if !conflicts.is_empty() {
            return Ok(RedoReceipt::NeedsConfirmation {
                conflicting_objects: conflicts,
            });
        }

        let redo_snapshots = self.redo_snapshots.lock().unwrap();
        let mut redone = Vec::new();
        for record in &in_scope {
            let snapshot = redo_snapshots
                .get(&record.action_id)
                .ok_or(RecoveryError::NoSuchAction)?
                .clone();
            self.apply_snapshot(monitor, token, &snapshot, &record.objects_touched)?;
            redone.push(record.action_id);
        }
        drop(redo_snapshots);

        let mut actions = self.actions.lock().unwrap();
        for a in actions.iter_mut() {
            if scope_ids.contains(&a.action_id) {
                a.status = ActionStatus::Committed;
            }
        }
        Ok(RedoReceipt::Targeted {
            redone_actions: redone,
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
