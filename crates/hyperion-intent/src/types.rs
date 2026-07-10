use hyperion_knowledge_graph::NodeId;
use serde::{Deserialize, Serialize};

/// docs/05 §4's `Intent.status` — see [02 §2] for the shared lifecycle
/// vocabulary this crate reuses rather than inventing its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentStatus {
    Proposed,
    Planned,
    Executing,
    Completed,
    Abandoned,
    Superseded,
}

/// docs/05 §4's `Intent`, narrowed per this crate's doc comment (no
/// per-slot `Slot`/`candidates` model — a single implicit "target" instead).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    #[serde(skip, default = "zero_node_id")]
    pub id: NodeId,
    pub raw_utterance: String,
    pub predicate: String,
    pub status: IntentStatus,
    pub priority: f32,
    pub confidence: f32,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    /// The single implicit target entity, if grounding succeeded — see
    /// this crate's doc comment on the narrowed slot model.
    pub grounded_entities: Vec<NodeId>,
    pub inferred_fields: Vec<String>,
    /// docs/05 §4's `IntentGraph.version` — meaningful only on the root
    /// node of a graph; non-root Intents leave this at `0`.
    pub version: u64,
    pub created_at: u64,
    pub updated_at: u64,
}

fn zero_node_id() -> NodeId {
    hyperion_storage::ObjectId(0)
}

/// docs/05 §4's `GraphMutation.op`, narrowed to the two the reconciliation
/// path in this crate actually implements — see this crate's doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationOp {
    Cancel,
    Amend,
    Supersede,
}

/// docs/05 §Interfaces' `submit(graph) -> ExecutionTicket`, standing in for
/// a real hand-off to [12 — Multi-Agent Coordination](../12-multi-agent-coordination.md)
/// (Phase 4, not built) — see this crate's doc comment.
#[derive(Debug, Clone)]
pub struct ExecutionTicket {
    pub root: NodeId,
    /// Leaves with no unmet `depends_on` — the frontier docs/05
    /// §Performance Analysis says gets submitted "as soon as it is ready
    /// rather than waiting on the full graph."
    pub ready_leaves: Vec<NodeId>,
}

/// The outcome of [`crate::IntentEngine::handle_utterance`] — docs/05 §4's
/// ask-vs-infer policy applied to reference resolution itself (§Algorithms
/// 4/5): a genuinely ambiguous grounding target escalates rather than
/// guesses.
#[derive(Debug, Clone)]
pub enum HandleOutcome {
    Submitted(NodeId),
    NeedsClarification {
        mention: String,
        candidates: Vec<NodeId>,
    },
}
