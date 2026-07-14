use std::collections::HashMap;

use hyperion_knowledge_graph::NodeId;

use crate::types::{ExpertiseEstimate, ExpertiseLevel};

/// docs/06 §Algorithms 3: "a per-session working set... produces new
/// bundles as incremental diffs against it," and §Recovery Mechanisms:
/// "thrashing is dampened with hysteresis: once an entity is included in
/// the working set, it requires a materially higher-scoring competitor... to
/// be displaced."
#[derive(Debug, Default)]
pub(crate) struct WorkingSet {
    pub(crate) entries: HashMap<NodeId, WorkingSetEntry>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WorkingSetEntry {
    pub(crate) last_included_at: u64,
    pub(crate) hits: u32,
}

impl WorkingSet {
    /// The hysteresis bonus applied to a candidate already resident in the
    /// working set — a simplification of "materially higher-scoring
    /// competitor" into a fixed additive margin rather than a computed
    /// significance test, adequate for damping single-session thrashing.
    pub(crate) const HYSTERESIS_BONUS: f32 = 0.1;

    pub(crate) fn interaction_frequency(&self, node_id: NodeId) -> f32 {
        self.entries
            .get(&node_id)
            .map(|e| e.hits as f32 / (e.hits as f32 + 1.0))
            .unwrap_or(0.0)
    }

    pub(crate) fn hysteresis_bonus(&self, node_id: NodeId) -> f32 {
        if self.entries.contains_key(&node_id) {
            Self::HYSTERESIS_BONUS
        } else {
            0.0
        }
    }

    pub(crate) fn record_inclusion(&mut self, node_id: NodeId, now: u64) {
        let entry = self.entries.entry(node_id).or_insert(WorkingSetEntry {
            last_included_at: now,
            hits: 0,
        });
        entry.last_included_at = now;
        entry.hits += 1;
    }

    pub(crate) fn active_node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.entries.keys().copied()
    }

    /// docs/06 §5.4's `ExpertiseEstimate`, narrowed to the one signal this
    /// crate actually has a source for: how broadly and how repeatedly this
    /// session's working set has been touched. Not docs/06's fuller
    /// vocabulary-complexity/capability-tier read (that needs a live signal
    /// from `hyperion-intent`/`hyperion-agent-runtime`, which this crate
    /// cannot depend on -- `hyperion-intent` already depends on
    /// `hyperion-context`, so the reverse edge would be a real cycle) --
    /// see this crate's doc comment. A session with no activity yet reports
    /// the same fixed `Novice`/zero-confidence estimate this method always
    /// returned before; only a session with real accumulated activity now
    /// gets a genuinely computed one.
    pub(crate) fn expertise_estimate(&self, domain: &str) -> ExpertiseEstimate {
        let distinct_entries = self.entries.len();
        let total_hits: u32 = self.entries.values().map(|e| e.hits).sum();

        let (level, confidence) = match distinct_entries + total_hits as usize {
            0 => (ExpertiseLevel::Novice, 0.0),
            1..=4 => (ExpertiseLevel::Novice, 0.2),
            5..=14 => (ExpertiseLevel::Intermediate, 0.5),
            15..=29 => (ExpertiseLevel::Advanced, 0.7),
            _ => (ExpertiseLevel::Expert, 0.85),
        };

        ExpertiseEstimate {
            domain: domain.to_string(),
            level,
            evidence: vec![format!(
                "{distinct_entries} distinct working-set entries, {total_hits} cumulative hits this session"
            )],
            confidence,
        }
    }
}
