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

/// docs/06 §5.4's own fuller Adaptive Complexity read, closed for real
/// (2026-07-18): "vocabulary complexity of recent Intents," "the Capability
/// tier the user has been reaching for," and "error-recovery behavior" all
/// need a live signal this crate cannot read by depending on its own real
/// source crate directly -- `hyperion-intent` already depends on this crate
/// (a reverse edge would be a real cycle), and `hyperion-agent-runtime`
/// closes an equally real cycle transitively, through `hyperion-netstack`
/// (see this crate's own doc comment). Rather than reversing either edge,
/// this crate defines the narrowed signal shape itself (the same "narrow
/// the type, never take the reverse dependency" precedent
/// `hyperion-explainability`'s own `RecoveryPointId`/`SensitivityClass` and
/// `hyperion-security`'s own `SensitivityHint` already established) and lets
/// whichever real caller already depends on both sides push a real,
/// already-computed sample in through [`crate::engine::ContextEngine::
/// record_expertise_signal`] -- `hyperion-intent::IntentEngine::handle_utterance`
/// for vocabulary complexity (via [`crate::vocabulary_complexity`], this
/// crate's own real scoring function, so both sides of that push agree on
/// what "complex" means), `hyperion-console::ConsoleSession` for the other
/// two (it already holds a real dispatch outcome and this session's own
/// `ContextEngine` handle at once).
#[derive(Debug, Clone, Copy)]
pub enum ExpertiseSignal {
    /// A real, computed vocabulary-complexity score for one recent utterance
    /// -- see [`crate::vocabulary_complexity`]. Expected in `[0.0, 1.0]`,
    /// but never asserted; an out-of-range caller-supplied value is folded
    /// in as-is rather than silently clamped, so a real, unexpected upstream
    /// bug shows up in the blended score instead of being hidden.
    VocabularyComplexity(f32),
    /// Which of docs/06's own two named Capability-tier reach patterns this
    /// turn actually took.
    CapabilityTierReach(CapabilityTierReach),
    /// Which of docs/06's own two named error-recovery patterns this turn
    /// actually took.
    ErrorRecovery(ErrorRecoveryPattern),
}

/// docs/06 §5.4's "the Capability tier the user has been reaching for (raw
/// API vs. guided workflow)".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityTierReach {
    /// A single Capability invoked directly, with no HTN decomposition --
    /// docs/06's own "raw API" reach.
    RawApi,
    /// A multi-task decomposed plan -- docs/06's own "guided workflow" reach.
    GuidedWorkflow,
}

/// docs/06 §5.4's "does the user self-correct with technical vocabulary, or
/// ask Hyperion to explain?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorRecoveryPattern {
    /// The user re-steered a task directly (e.g. a real `/redo` with more
    /// precise instructions) rather than asking for an explanation.
    SelfCorrected,
    /// The user asked Hyperion to explain instead (e.g. a real `/teach`).
    AskedForExplanation,
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
