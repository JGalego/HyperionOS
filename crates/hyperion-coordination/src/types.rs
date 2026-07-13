use std::collections::HashMap;

use hyperion_agent_runtime::TrustTier;
use hyperion_explainability::ExplanationId;
use hyperion_knowledge_graph::NodeId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Unassigned,
    Claimed,
    InProgress,
    Blocked,
    Done,
    Failed,
}

/// docs/12 §4.1's `TaskNode`, narrowed to what this crate's synchronous
/// allocate-and-execute pass needs — see this crate's doc comment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    pub task_id: NodeId,
    pub description: String,
    pub required_capabilities: Vec<String>,
    pub required_trust_tier: TrustTier,
    pub assigned_agent: Option<u64>,
    pub status: TaskStatus,
    pub dependencies: Vec<NodeId>,
    pub base_version: u64,
    /// How many times this task has been (re)allocated after a failure —
    /// docs/12 §5.4's bounded retry.
    pub attempts: u32,
    /// The real capability dispatch's own returned value, once `status` reaches [`TaskStatus::
    /// Done`] — `None` until then, and left `None` on failure/blocking. A real, previously-shipped
    /// bug this field fixes: [`crate::CoordinationSession::allocate`] used to discard
    /// `InvokeOutcome::Result`'s own value outright (`InvokeOutcome::Result(_)`), so a real
    /// capability's real output (a drafted document, a research summary) was thrown away the
    /// instant it came back — nothing downstream, not even this crate's own caller, could ever
    /// see it. `Option<serde_json::Value>` rather than a typed shape because different
    /// capabilities return different shapes (`document.draft`'s `"draft"`, `web.search`'s
    /// `"results"`/`"note"`) and this crate has no per-capability output schema to type it against.
    #[serde(default)]
    pub result: Option<serde_json::Value>,
}

/// docs/12 §4.3's `ConflictRecord`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictKind {
    ConcurrentWrite,
    ContradictorySubplan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictResolution {
    Pending,
    AutoMerged,
    CoordinatorResolved,
    UserResolved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictRecord {
    pub conflict_id: u64,
    pub key: String,
    pub claimants: Vec<u64>,
    pub kind: ConflictKind,
    pub resolution: ConflictResolution,
}

/// A named, specific blocker — docs/12 §8's worked example: "Legal is
/// stuck — the filing requires a decision only you can make," never a
/// silent stall.
#[derive(Debug, Clone)]
pub struct Escalation {
    pub task_id: NodeId,
    pub reason: String,
}

/// docs/12 §4.1's `SharedPlan` (Blackboard), narrowed per this crate's doc
/// comment — `facts` generalizes docs/12's `object_ref`-keyed writes to a
/// named key, since this crate has no real Semantic Object write path of
/// its own (writes here are plan-local facts like "product_name", not
/// Knowledge Graph mutations).
#[derive(Debug, Clone)]
pub struct SharedPlan {
    pub session_id: u64,
    pub root_intent: NodeId,
    /// The real utterance behind `root_intent` (`hyperion_intent::types::Intent::raw_utterance`),
    /// captured once at [`crate::CoordinationSession::create_session`] time so
    /// [`crate::CoordinationSession::allocate`] can give each task's real capability dispatch
    /// genuine context to work with (what the user actually asked for), not just the bare task
    /// predicate name it had before.
    pub root_utterance: String,
    pub version: u64,
    pub nodes: Vec<TaskNode>,
    pub participants: Vec<u64>,
    pub conflicts: Vec<ConflictRecord>,
    pub facts: HashMap<String, (u64, serde_json::Value)>,
}

/// The outcome of one [`crate::CoordinationSession::propose_write`] call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WriteOutcome {
    Accepted { new_version: u64 },
    Conflict(ConflictRecord),
}

/// One allocation decision made during a [`crate::CoordinationSession::allocate`]
/// pass, returned for callers/tests to inspect what happened this tick.
/// `explanation_id` resolves via [`crate::CoordinationSession::explanation`]
/// to the real `hyperion-explainability` record this dispatch produced.
#[derive(Debug, Clone)]
pub struct AllocationRecord {
    pub task_id: NodeId,
    pub agent_instance: u64,
    pub outcome: TaskStatus,
    pub explanation_id: ExplanationId,
}
