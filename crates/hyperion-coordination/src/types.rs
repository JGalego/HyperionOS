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

/// docs/998-roadmap.md's Backlog "Protect the Human" item: "no declared judgment/taste/empathy/
/// context boundary, distinct from 'risky.'" `hyperion-security`'s existing consent gate triggers
/// on irreversibility/cost/security — a different axis from "this decision is a matter of taste
/// or empathy and deserves more human involvement regardless of how reversible it is." See
/// [`crate::catalog::judgment_class_for`] for the real classification this crate assigns per task
/// predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JudgmentClass {
    /// A routine, mechanical task — no particular reason for extra human involvement beyond
    /// whatever `hyperion-security`'s own risk axis already asks for.
    Mechanical,
    /// A matter of taste, brand, or empathy — the roadmap's own worked example is branding a
    /// startup vs. filing its paperwork, dispatched identically today despite one being a
    /// judgment call and the other being mechanical.
    TasteOrEmpathy,
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
    /// Extra, user-supplied steering text for this task's *next* real dispatch, set by
    /// [`crate::CoordinationSession::amend_task`] -- `hyperion-console`'s own `/redo <task>
    /// <extra instructions>` meta-command is the real caller. Threaded into
    /// [`crate::CoordinationSession::prepare_dispatches`]'s own args and, from there, into the
    /// real prompt `hyperion_agent_runtime::AgentRuntime::dispatch_document_draft`/
    /// `dispatch_market_research` build -- not cleared after use, so a second redo without new
    /// instructions still carries the last real steering text forward rather than silently
    /// reverting to none.
    #[serde(default)]
    pub extra_context: Option<String>,
    /// docs/998-roadmap.md's Backlog "Protect the Human" item — see [`JudgmentClass`]. Set once,
    /// at [`crate::CoordinationEngine::create_session`] time, from this task's own real predicate
    /// via [`crate::catalog::judgment_class_for`].
    #[serde(default = "default_judgment_class")]
    pub judgment_class: JudgmentClass,
}

fn default_judgment_class() -> JudgmentClass {
    JudgmentClass::Mechanical
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
    /// docs/12 §12's "object-affinity" plan partitioning: a real, distinct version counter per
    /// connected group of tasks (see [`crate::engine::task_partition_key`]), rather than one
    /// global counter every task-status change on the whole plan bumped regardless of which task
    /// changed — the exact write-contention hotspot §12 names ("the single Shared Plan can become
    /// a write-contention hotspot... partitioned by object-affinity so unrelated branches...
    /// rarely contend on the same version counter"). Keyed by [`crate::engine::task_partition_key`]'s
    /// own output; a partition with no entry yet has implicitly never changed (version 0) — see
    /// [`crate::CoordinationSession::partition_version`].
    pub partition_versions: HashMap<String, u64>,
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
