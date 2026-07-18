use std::collections::HashMap;

use hyperion_knowledge_graph::NodeId;

use crate::types::{
    CapabilityTierReach, ErrorRecoveryPattern, ExpertiseEstimate, ExpertiseLevel, ExpertiseSignal,
};

/// docs/06 §Algorithms 3: "a per-session working set... produces new
/// bundles as incremental diffs against it," and §Recovery Mechanisms:
/// "thrashing is dampened with hysteresis: once an entity is included in
/// the working set, it requires a materially higher-scoring competitor... to
/// be displaced."
#[derive(Debug, Default)]
pub(crate) struct WorkingSet {
    pub(crate) entries: HashMap<NodeId, WorkingSetEntry>,
    /// Real, pushed [`ExpertiseSignal::VocabularyComplexity`] samples -- see
    /// [`Self::record_expertise_signal`].
    vocabulary_complexity_samples: Vec<f32>,
    /// Real, pushed [`ExpertiseSignal::CapabilityTierReach`] samples.
    capability_tier_reaches: Vec<CapabilityTierReach>,
    /// Real, pushed [`ExpertiseSignal::ErrorRecovery`] samples.
    error_recovery_events: Vec<ErrorRecoveryPattern>,
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

    /// Accumulates one real, caller-pushed Adaptive Complexity sample -- see
    /// [`crate::types::ExpertiseSignal`]'s own doc comment for why this crate never reads these
    /// signals itself.
    pub(crate) fn record_expertise_signal(&mut self, signal: ExpertiseSignal) {
        match signal {
            ExpertiseSignal::VocabularyComplexity(score) => {
                self.vocabulary_complexity_samples.push(score)
            }
            ExpertiseSignal::CapabilityTierReach(reach) => self.capability_tier_reaches.push(reach),
            ExpertiseSignal::ErrorRecovery(pattern) => self.error_recovery_events.push(pattern),
        }
    }

    /// docs/06 §5.4's `ExpertiseEstimate`, now blending all four real Adaptive Complexity
    /// signals docs/06 names: this session's own working-set breadth/repetition (always
    /// available), plus whichever of vocabulary complexity, Capability-tier reach, and
    /// error-recovery pattern a real caller has actually pushed via
    /// [`Self::record_expertise_signal`] (see [`crate::types::ExpertiseSignal`]'s own doc
    /// comment for why this crate can't compute those three itself). Each signal only
    /// contributes when this session genuinely has a sample for it -- never fabricated -- and
    /// `evidence` names exactly which signals actually informed the result, so a caller can tell
    /// a narrow, working-set-only estimate from a fuller, multi-signal one. A session with
    /// neither working-set activity nor any pushed sample reports the same fixed
    /// `Novice`/zero-confidence estimate this method always returned before.
    pub(crate) fn expertise_estimate(&self, domain: &str) -> ExpertiseEstimate {
        let distinct_entries = self.entries.len();
        let total_hits: u32 = self.entries.values().map(|e| e.hits).sum();
        let mut score = (distinct_entries + total_hits as usize) as f32;
        let mut evidence = vec![format!(
            "{distinct_entries} distinct working-set entries, {total_hits} cumulative hits this session"
        )];
        let mut signal_count = 0u32;

        if !self.vocabulary_complexity_samples.is_empty() {
            let avg = self.vocabulary_complexity_samples.iter().sum::<f32>()
                / self.vocabulary_complexity_samples.len() as f32;
            // A fully-complex average (1.0) contributes as much as 10 extra working-set
            // touches; a fully-simple average (0.0) contributes nothing.
            score += avg * 10.0;
            evidence.push(format!(
                "average vocabulary complexity {avg:.2} over {} recent utterance(s)",
                self.vocabulary_complexity_samples.len()
            ));
            signal_count += 1;
        }

        if !self.capability_tier_reaches.is_empty() {
            let total = self.capability_tier_reaches.len();
            let raw_api_count = self
                .capability_tier_reaches
                .iter()
                .filter(|r| **r == CapabilityTierReach::RawApi)
                .count();
            // Reaching directly for a raw Capability (skipping a guided, decomposed workflow)
            // is docs/06's own named signal of higher expertise.
            score += (raw_api_count as f32 / total as f32) * 10.0;
            evidence.push(format!(
                "reached for a raw Capability directly {raw_api_count}/{total} time(s) rather \
                 than a guided workflow"
            ));
            signal_count += 1;
        }

        if !self.error_recovery_events.is_empty() {
            let total = self.error_recovery_events.len();
            let self_corrected_count = self
                .error_recovery_events
                .iter()
                .filter(|e| **e == ErrorRecoveryPattern::SelfCorrected)
                .count();
            // Self-correcting with more precise steering is docs/06's own named signal of
            // higher expertise; asking for an explanation instead nudges the score down rather
            // than simply contributing nothing, since it's a real, opposite-direction signal.
            let self_corrected_ratio = self_corrected_count as f32 / total as f32;
            score += self_corrected_ratio * 10.0 - (1.0 - self_corrected_ratio) * 5.0;
            evidence.push(format!(
                "self-corrected with more precise steering {self_corrected_count}/{total} \
                 time(s) rather than asking for an explanation"
            ));
            signal_count += 1;
        }

        let (level, base_confidence) = match score.max(0.0) as usize {
            0 => (ExpertiseLevel::Novice, 0.0),
            1..=4 => (ExpertiseLevel::Novice, 0.2),
            5..=14 => (ExpertiseLevel::Intermediate, 0.5),
            15..=29 => (ExpertiseLevel::Advanced, 0.7),
            _ => (ExpertiseLevel::Expert, 0.85),
        };
        // More independent real signals contributing means more confidence in the result,
        // capped well short of certainty.
        let confidence = (base_confidence + 0.05 * signal_count as f32).min(0.95);

        ExpertiseEstimate {
            domain: domain.to_string(),
            level,
            evidence,
            confidence,
        }
    }
}
