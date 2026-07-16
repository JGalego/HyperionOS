//! docs/18 §10/§13's "rolling Brier score per Agent/Capability... feeding an alert if an Agent's
//! stated confidence systematically diverges from observed outcomes," proven through the real
//! `ExplanationStore` API end to end -- not just the pure scoring function in isolation.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_explainability::{ConfidenceMethod, ConfidenceScore, ControlState, ExplanationStore};

fn setup() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    ExplanationStore,
) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    (monitor, root, ExplanationStore::new())
}

fn record_completed_action(
    monitor: &CapabilityMonitor,
    root: &hyperion_capability::CapabilityToken,
    store: &ExplanationStore,
    agent_id: u64,
    capability_ref: &str,
    confidence: f32,
    final_state: ControlState,
) {
    let id = store
        .begin(monitor, root, 1, 7, agent_id, capability_ref, vec![], 1_000)
        .unwrap();
    store
        .set_confidence(
            monitor,
            root,
            id,
            ConfidenceScore {
                value: confidence,
                method: ConfidenceMethod::Heuristic,
            },
            vec![],
        )
        .unwrap();
    store.transition(monitor, root, id, final_state).unwrap();
}

#[test]
fn no_records_at_all_for_a_pair_gives_no_score() {
    let (_monitor, _root, store) = setup();
    assert!(store.calibration_score(42, "document.draft").is_none());
}

#[test]
fn a_real_agent_that_is_always_right_scores_a_perfect_zero() {
    let (monitor, root, store) = setup();
    record_completed_action(
        &monitor,
        &root,
        &store,
        42,
        "document.draft",
        1.0,
        ControlState::Completed,
    );
    record_completed_action(
        &monitor,
        &root,
        &store,
        42,
        "document.draft",
        0.0,
        ControlState::RolledBack,
    );

    let score = store.calibration_score(42, "document.draft").unwrap();
    assert_eq!(score.brier_score, 0.0);
    assert_eq!(score.sample_count, 2);
    assert!(!score.alert);
}

#[test]
fn a_real_agent_confidently_wrong_repeatedly_triggers_a_real_alert() {
    let (monitor, root, store) = setup();
    for _ in 0..6 {
        record_completed_action(
            &monitor,
            &root,
            &store,
            42,
            "document.draft",
            0.95,
            ControlState::RolledBack,
        );
    }

    let score = store.calibration_score(42, "document.draft").unwrap();
    assert_eq!(score.sample_count, 6);
    assert!(score.brier_score > 0.25);
    assert!(
        score.alert,
        "a real Agent whose stated confidence never matches its real outcome must alert"
    );
}

#[test]
fn a_still_in_flight_action_does_not_count_toward_calibration_yet() {
    let (monitor, root, store) = setup();
    let id = store
        .begin(&monitor, &root, 1, 7, 42, "document.draft", vec![], 1_000)
        .unwrap();
    store
        .set_confidence(
            &monitor,
            &root,
            id,
            ConfidenceScore {
                value: 0.9,
                method: ConfidenceMethod::Heuristic,
            },
            vec![],
        )
        .unwrap();
    store
        .transition(&monitor, &root, id, ControlState::Executing)
        .unwrap();
    // Never reaches Completed/RolledBack.

    assert!(
        store.calibration_score(42, "document.draft").is_none(),
        "an action with no real terminal outcome yet must not be scored"
    );
}

#[test]
fn different_capabilities_for_the_same_agent_are_scored_independently() {
    let (monitor, root, store) = setup();
    record_completed_action(
        &monitor,
        &root,
        &store,
        42,
        "document.draft",
        1.0,
        ControlState::Completed,
    );
    record_completed_action(
        &monitor,
        &root,
        &store,
        42,
        "web.search",
        0.9,
        ControlState::RolledBack,
    );

    let draft_score = store.calibration_score(42, "document.draft").unwrap();
    assert_eq!(draft_score.brier_score, 0.0);

    let search_score = store.calibration_score(42, "web.search").unwrap();
    assert!(search_score.brier_score > draft_score.brier_score);
}
