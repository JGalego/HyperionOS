use std::collections::VecDeque;

use hyperion_knowledge_graph::NodeId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryTier {
    Episodic,
    Semantic,
    Procedural,
    LongTerm,
}

impl MemoryTier {
    pub(crate) fn as_object_type(&self) -> &'static str {
        match self {
            MemoryTier::Episodic => "memory_episodic",
            MemoryTier::Semantic => "memory_semantic",
            MemoryTier::Procedural => "memory_procedural",
            MemoryTier::LongTerm => "memory_long_term",
        }
    }

    /// docs/08 §5.2's per-tier recency half-life `τ_tier`, in seconds.
    /// Long-Term records don't decay by construction (they're the
    /// consolidation terminus) — represented as an effectively-infinite τ.
    pub(crate) fn tau_seconds(&self) -> f64 {
        match self {
            MemoryTier::Episodic => 7.0 * 24.0 * 3600.0, // weeks
            MemoryTier::Semantic | MemoryTier::Procedural => 30.0 * 24.0 * 3600.0, // months
            MemoryTier::LongTerm => f64::INFINITY,
        }
    }
}

/// docs/08 §4's `MemoryRecord` envelope, shared by every persisted tier.
/// The Knowledge Graph node's `metadata` *is* this struct, serialized —
/// see [`crate::engine::MemoryEngine`].
fn zero_node_id() -> NodeId {
    // `NodeId` is a `type` alias for `hyperion_storage::ObjectId` — a type
    // alias only aliases the type namespace, not the tuple-struct
    // constructor in the value namespace, so it must be constructed via
    // its original name.
    hyperion_storage::ObjectId(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    /// Not stored in the node's metadata — it *is* the node's own
    /// `hyperion_knowledge_graph::NodeId`, filled in by
    /// [`crate::MemoryEngine`] after reading it back, never round-tripped
    /// through JSON.
    #[serde(skip, default = "zero_node_id")]
    pub id: NodeId,
    pub tier: MemoryTier,
    pub content: serde_json::Value,
    pub embedding: Option<Vec<f32>>,
    pub created_at: u64,
    pub last_accessed_at: u64,
    pub access_count: u32,
    pub importance: f32,
    pub decay_score: f32,
    pub pinned: bool,
    pub provenance: Vec<NodeId>,
    /// SoftDelete — see this crate's doc comment. Filtered from
    /// [`crate::MemoryFilter`] results by default.
    pub erased: bool,
    /// docs/08 §5.3's decay funnel stage 4 ("Dormant") — a visibility flag,
    /// not a storage-tier migration; see this crate's doc comment.
    pub dormant: bool,
}

/// docs/998-roadmap.md's Backlog "Protect the Human" item: "no signal exists for 'you've
/// delegated this kind of task N times this month, want to do the next one yourself?'" — a real
/// count, never a decision (see [`crate::engine::MemoryEngine::count_procedural_delegations`]).
#[derive(Debug, Clone, PartialEq)]
pub struct DelegationCount {
    pub entity_key: String,
    pub count: usize,
    pub window_start: u64,
}

/// docs/08 §4: "not a `MemoryRecord`; RAM-resident only" — never persisted
/// to the Knowledge Graph, discarded at session close after distillation
/// (§5.1).
#[derive(Debug, Clone)]
pub struct WorkingMemory {
    pub session_id: String,
    turns: VecDeque<String>,
    capacity: usize,
}

impl WorkingMemory {
    pub fn new(session_id: impl Into<String>, capacity: usize) -> Self {
        WorkingMemory {
            session_id: session_id.into(),
            turns: VecDeque::new(),
            capacity,
        }
    }

    /// Bounded ring buffer, evicted oldest-first — docs/08 §4.
    pub fn push_turn(&mut self, turn: impl Into<String>) {
        if self.turns.len() >= self.capacity {
            self.turns.pop_front();
        }
        self.turns.push_back(turn.into());
    }

    pub fn turns(&self) -> impl Iterator<Item = &str> {
        self.turns.iter().map(|s| s.as_str())
    }
}
