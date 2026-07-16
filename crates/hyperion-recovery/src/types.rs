use hyperion_knowledge_graph::{NodeId, NodeRecord};

pub type RecoveryPointId = u64;
pub type ActionId = u64;

/// docs/33 §4's `Trigger`, narrowed to the variants this workspace's
/// existing crates can actually originate — no `PreUpdate` payload type
/// exists yet (Phase 10's Update System), so it carries no data here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Automatic,
    UserRequested,
    PreRiskyAction,
    PreUpdate,
    PreAgentRun { agent_run_id: u64 },
    PreGoalFork { goal_id: u64 },
}

/// docs/33 §4's `RecoveryPoint`. docs/33 frames this as "a durable,
/// timestamped *reference*, not a copy of data" — true for the real
/// store's native MVCC/content-addressing, but
/// `hyperion-knowledge-graph` doesn't yet expose a historical-version
/// read API (see this crate's doc comment), so
/// [`crate::service::RecoveryService`] captures a bounded copy of just
/// the objects a triggering action is about to touch, not a whole-graph
/// cut. Still cheap, at the scale of one action's `objects_touched`.
#[derive(Debug, Clone)]
pub struct RecoveryPoint {
    pub id: RecoveryPointId,
    pub created_at: u64,
    pub trigger: Trigger,
    pub pinned: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionStatus {
    Committed,
    InFlight,
    Aborted,
    /// Reverted by [`crate::service::RecoveryService::undo`] -- distinct
    /// from [`Self::Aborted`] (an action that never took effect) because an
    /// undone action's effects *did* happen and are recorded, real, and
    /// redoable via [`crate::service::RecoveryService::redo`]; an aborted
    /// one never ran to begin with and has nothing to redo.
    Undone,
}

/// docs/33 §4's `ActionRecord` — the "undo record." No separate undo-
/// stack type exists in the doc either; undo is resolved dynamically by
/// querying the journal, as implemented in
/// [`crate::service::RecoveryService::undo`].
#[derive(Debug, Clone)]
pub struct ActionRecord {
    pub action_id: ActionId,
    pub agent_run_id: Option<u64>,
    /// docs/33 §4's `UndoScope::Session` key — real once a caller has a real session id to tag
    /// with (see [`UndoScope::Session`]'s own doc comment for the first real one,
    /// `hyperion-coordination`'s own `SharedPlan.session_id`). `None` for every action recorded
    /// by a caller with no session concept of its own.
    pub session_id: Option<u64>,
    /// docs/33 §4's `UndoScope::Goal` key — see [`UndoScope::Goal`]'s own doc comment.
    pub goal_id: Option<NodeId>,
    pub recovery_point_before: RecoveryPointId,
    pub objects_touched: Vec<NodeId>,
    pub status: ActionStatus,
    pub created_at: u64,
    pub note: String,
}

/// docs/33 §4's `UndoScope`. `Session`/`Goal` were previously narrowed away — "neither concept
/// has a first-class id anywhere in this workspace" — but that premise is now false:
/// `hyperion_coordination::types::SharedPlan` has both a real `session_id: u64` and a real
/// `root_intent: NodeId` (the goal). [`crate::service::RecoveryService::record_action_started_with_scope`]
/// is the real, tagged counterpart to [`crate::service::RecoveryService::record_action_started`]
/// (which still tags neither, for every caller with no session/goal concept of its own) — see
/// that method's own doc comment for the one real caller today
/// (`hyperion_coordination::CoordinationSession`, via `Self::with_recovery`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoScope {
    SingleAction(ActionId),
    AgentRun(u64),
    /// Every action recorded under this real `hyperion-coordination` session id.
    Session(u64),
    /// Every action recorded under this real root Intent (goal) id.
    Goal(NodeId),
    /// Every action recorded against this recovery point, not just one.
    Global(RecoveryPointId),
}

/// docs/33 §3's `UndoReceipt` — `NeedsConfirmation` never auto-applies.
#[derive(Debug, Clone)]
pub enum UndoReceipt {
    Targeted { undone_actions: Vec<ActionId> },
    NeedsConfirmation { conflicting_objects: Vec<NodeId> },
    NothingToUndo,
}

/// docs/33 §3's `redo(scope)` counterpart to [`UndoReceipt`] — same shape,
/// same "never silently overwrite a real conflict" guarantee, mirrored for
/// re-applying an already-undone action's effects.
#[derive(Debug, Clone)]
pub enum RedoReceipt {
    Targeted { redone_actions: Vec<ActionId> },
    NeedsConfirmation { conflicting_objects: Vec<NodeId> },
    NothingToRedo,
}

pub(crate) type Snapshot = Vec<(NodeId, Option<NodeRecord>)>;

/// The real *why* behind a rollback -- docs/998-roadmap.md's Self-Sustaining pillar's own named
/// gap: "`hyperion-recovery` learning from what it rolls back. Still purely reactive; no
/// mechanism connects a rollback's cause to a future decision." `reason` is a short, plain-
/// language label a caller already has at the moment it decides to roll back (e.g. a health
/// threshold breach); `detail` is whatever real, structured data justified that reason (e.g. the
/// actual health numbers), kept as-is rather than flattened into more strings — a future
/// decision point can read it back verbatim via
/// [`crate::service::RecoveryService::rollback_causes`].
#[derive(Debug, Clone)]
pub struct RollbackCause {
    pub reason: String,
    pub detail: serde_json::Value,
}

/// One real, remembered rollback -- what [`crate::service::RecoveryService::rollback_causes`]
/// returns. `subject` mirrors whatever caller-defined string identified the thing rolled back
/// (this crate has no `UpdateSubject`/etc. of its own to reuse — see
/// `hyperion-update::orchestrator::UpdateOrchestrator::update_rollback_with_cause`, the real
/// caller that supplies one).
#[derive(Debug, Clone)]
pub struct RecordedRollback {
    pub recovery_point_id: RecoveryPointId,
    pub subject: String,
    pub cause: RollbackCause,
    pub created_at: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("no such recovery point")]
    NoSuchRecoveryPoint,
    #[error("no such action record")]
    NoSuchAction,
    #[error("knowledge graph error: {0}")]
    Graph(#[from] hyperion_knowledge_graph::GraphError),
    #[error("agent runtime error: {0}")]
    Agent(#[from] hyperion_agent_runtime::AgentError),
    #[error("memory error: {0}")]
    Memory(#[from] hyperion_memory::MemoryError),
}
