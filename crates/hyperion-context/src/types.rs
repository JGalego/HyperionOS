use hyperion_knowledge_graph::NodeId;

/// Stands in for a real Intent until [05 — Intent Engine](../05-intent-engine.md)
/// exists (Phase 3) — see this crate's doc comment. `mentions` are
/// underspecified entity references from the utterance (docs/06's "the
/// API"); `anchors` are Semantic Objects already known to be relevant
/// (e.g. the repository currently open), which seed the traversal in
/// [`crate::engine::ContextEngine::assemble`].
#[derive(Debug, Clone)]
pub struct Scope {
    pub intent_id: String,
    pub session_id: String,
    pub mentions: Vec<String>,
    pub anchors: Vec<NodeId>,
}

/// docs/06 §Data Structures' `ContextBundle.budget`.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    pub max_tokens: usize,
    pub max_entries_per_category: usize,
}

impl Default for Budget {
    /// docs/06 §Performance Analysis: "Default budget is a core bundle
    /// around 4K tokens."
    fn default() -> Self {
        Budget {
            max_tokens: 4096,
            max_entries_per_category: 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InclusionMode {
    Full,
    Summary,
    Reference,
}

/// docs/06 §Data Structures' `ContextEntry`.
#[derive(Debug, Clone)]
pub struct ContextEntry {
    pub category: String,
    pub node_id: NodeId,
    pub inclusion_mode: InclusionMode,
    /// Full metadata for `Full`, a truncated stand-in for `Summary` (see
    /// this crate's doc comment on deferred real summarization), and
    /// `serde_json::Value::Null` for `Reference` (a pointer only — the
    /// receiving Agent calls `KnowledgeGraph::get` to expand it).
    pub content: serde_json::Value,
    pub relevance_score: f32,
    pub source_signal: Vec<String>,
    /// The node's generation (docs/06 `staleness.generation`) at the moment
    /// this entry was assembled — see [`hyperion_knowledge_graph::KnowledgeGraph::generation`].
    pub generation: u64,
    pub captured_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExpertiseLevel {
    Novice,
    Intermediate,
    Advanced,
    Expert,
}

/// docs/06 §Data Structures' `ExpertiseEstimate` — see this crate's doc
/// comment's "Adaptive Complexity" deferral for why this is currently
/// always a fixed, honest stub rather than a computed signal.
#[derive(Debug, Clone)]
pub struct ExpertiseEstimate {
    pub domain: String,
    pub level: ExpertiseLevel,
    pub evidence: Vec<String>,
    pub confidence: f32,
}

/// docs/06 §Data Structures' `ContextBundle`.
#[derive(Debug, Clone)]
pub struct ContextBundle {
    pub bundle_id: u64,
    pub scope: Scope,
    pub entries: Vec<ContextEntry>,
    pub assembled_at: u64,
    pub budget: Budget,
    pub expertise_signal: ExpertiseEstimate,
}
