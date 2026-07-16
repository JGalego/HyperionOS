//! docs/18-explainability-and-trust.md §10/§13's "rolling Brier score per Agent/Capability...
//! feeding an alert if an Agent's stated confidence systematically diverges from observed
//! outcomes" — closing this crate's own previously-named "Rolling Brier-score calibration
//! tracking (§10)" gap for real: [`crate::ExplanationStore::calibration_score`] computes it over
//! this crate's own already-real records, no new store or background job needed.

use crate::types::{CalibrationScore, ControlState, ExplanationRecord};

/// A reference point, not a caller-tunable knob: a forecaster who is always exactly 75%
/// confident and right exactly 75% of the time scores `0.1875` — comfortably below this
/// threshold; one whose stated confidence bears no relation to outcome at all (always 50%
/// confident) scores `0.25` — right at it. Crossing this threshold is a real, computed signal
/// that confidence and outcome have meaningfully diverged, not merely ordinary variance.
const ALERT_THRESHOLD: f32 = 0.25;
/// Below this many real, terminal, confidence-scored records, a `brier_score` is real but not yet
/// a reliable enough signal to alert on — an Agent's first couple of actions shouldn't trip an
/// alert off a tiny sample.
const MIN_SAMPLES_FOR_ALERT: usize = 5;

/// Computes [`CalibrationScore`] for one `(agent_id, capability_ref)` pair over `records` — every
/// real record in `records` matching that pair with both a real `confidence` and a real terminal
/// `control_state` (`Completed` or `RolledBack`; `Proposed`/`Executing`/`Interrupted`/`Modified`
/// have no real outcome yet to score against, matching [`crate::ExplanationStore::incomplete`]'s
/// own convention for what counts as resolved). `None` if there are no such records at all —
/// nothing to compute a score over, not a real zero.
pub(crate) fn calibration_score(
    records: &[ExplanationRecord],
    agent_id: u64,
    capability_ref: &str,
) -> Option<CalibrationScore> {
    let outcomes: Vec<(f32, f32)> = records
        .iter()
        .filter(|record| record.agent_id == agent_id && record.capability_ref == capability_ref)
        .filter_map(|record| {
            let confidence = record.confidence?.value;
            let outcome = match record.control_state {
                ControlState::Completed => 1.0,
                ControlState::RolledBack => 0.0,
                ControlState::Proposed
                | ControlState::Executing
                | ControlState::Interrupted
                | ControlState::Modified => return None,
            };
            Some((confidence, outcome))
        })
        .collect();

    if outcomes.is_empty() {
        return None;
    }

    let sample_count = outcomes.len();
    let sum_squared_error: f32 = outcomes
        .iter()
        .map(|(confidence, outcome)| (confidence - outcome).powi(2))
        .sum();
    let brier_score = sum_squared_error / sample_count as f32;

    Some(CalibrationScore {
        brier_score,
        sample_count,
        alert: sample_count >= MIN_SAMPLES_FOR_ALERT && brier_score > ALERT_THRESHOLD,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ConfidenceMethod, ConfidenceScore};

    fn record(
        agent_id: u64,
        capability_ref: &str,
        confidence: f32,
        control_state: ControlState,
    ) -> ExplanationRecord {
        ExplanationRecord {
            id: 0,
            action_id: 0,
            triggering_intent_id: 0,
            agent_id,
            capability_ref: capability_ref.to_string(),
            created_at: 0,
            reasoning_chain: Vec::new(),
            evidence: Vec::new(),
            confidence: Some(ConfidenceScore {
                value: confidence,
                method: ConfidenceMethod::Heuristic,
            }),
            alternatives: Vec::new(),
            undo_ref: None,
            trust_boundary_span: Vec::new(),
            privacy_class: None,
            parent_records: Vec::new(),
            child_records: Vec::new(),
            control_state,
        }
    }

    #[test]
    fn no_matching_records_returns_none() {
        let records = vec![record(1, "web.search", 0.9, ControlState::Completed)];
        assert!(calibration_score(&records, 2, "web.search").is_none());
    }

    #[test]
    fn non_terminal_records_are_excluded_from_the_score() {
        let records = vec![
            record(1, "web.search", 0.9, ControlState::Proposed),
            record(1, "web.search", 0.9, ControlState::Executing),
            record(1, "web.search", 0.9, ControlState::Interrupted),
            record(1, "web.search", 0.9, ControlState::Modified),
        ];
        assert!(calibration_score(&records, 1, "web.search").is_none());
    }

    #[test]
    fn records_with_no_confidence_are_excluded() {
        let mut r = record(1, "web.search", 0.9, ControlState::Completed);
        r.confidence = None;
        assert!(calibration_score(&[r], 1, "web.search").is_none());
    }

    #[test]
    fn a_perfectly_calibrated_agent_scores_zero() {
        let records = vec![
            record(1, "web.search", 1.0, ControlState::Completed),
            record(1, "web.search", 0.0, ControlState::RolledBack),
        ];
        let score = calibration_score(&records, 1, "web.search").unwrap();
        assert_eq!(score.brier_score, 0.0);
        assert_eq!(score.sample_count, 2);
        assert!(!score.alert, "a perfect score must never alert");
    }

    #[test]
    fn a_confidently_wrong_agent_scores_high_and_alerts_with_enough_samples() {
        let records: Vec<_> = (0..6)
            .map(|_| record(1, "web.search", 0.95, ControlState::RolledBack))
            .collect();
        let score = calibration_score(&records, 1, "web.search").unwrap();
        assert!(score.brier_score > ALERT_THRESHOLD);
        assert_eq!(score.sample_count, 6);
        assert!(score.alert);
    }

    #[test]
    fn a_confidently_wrong_agent_with_too_few_samples_does_not_alert() {
        let records: Vec<_> = (0..3)
            .map(|_| record(1, "web.search", 0.95, ControlState::RolledBack))
            .collect();
        let score = calibration_score(&records, 1, "web.search").unwrap();
        assert!(score.brier_score > ALERT_THRESHOLD);
        assert!(
            !score.alert,
            "too few samples to trust the signal must never alert, even with a bad score"
        );
    }

    #[test]
    fn different_agents_and_capabilities_are_scored_independently() {
        let records = vec![
            record(1, "web.search", 1.0, ControlState::Completed),
            record(2, "web.search", 0.0, ControlState::Completed),
            record(1, "document.draft", 0.0, ControlState::Completed),
        ];
        let agent1_web_search = calibration_score(&records, 1, "web.search").unwrap();
        assert_eq!(agent1_web_search.sample_count, 1);
        assert_eq!(agent1_web_search.brier_score, 0.0);

        let agent2_web_search = calibration_score(&records, 2, "web.search").unwrap();
        assert_eq!(agent2_web_search.sample_count, 1);
        assert_eq!(agent2_web_search.brier_score, 1.0);
    }
}
