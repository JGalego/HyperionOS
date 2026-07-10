//! docs/18 §5's explain-then-commit ordering and §9's completeness
//! invariant: no effect survives without a matching completed record.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_explainability::{
    Alternative, ConfidenceMethod, ConfidenceScore, ControlState, EvidenceRef, ExplanationStore,
    ReasoningStep,
};

fn setup() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    ExplanationStore,
) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    (monitor, root, ExplanationStore::new())
}

#[test]
fn a_completed_action_has_a_full_reasoning_chain_and_confidence() {
    let (monitor, root, store) = setup();
    let id = store
        .begin(&monitor, &root, 1, 7, 42, "document.draft", vec![1], 1_000)
        .unwrap();

    store
        .append_step(
            &monitor,
            &root,
            id,
            ReasoningStep {
                step_index: 0,
                description: "gathered evidence".to_string(),
                capability_ref: Some("web.search".to_string()),
                inputs_ref: Vec::new(),
                output_ref: None,
            },
            vec![EvidenceRef {
                object_id: hyperion_storage::ObjectId(9),
                excerpt_or_summary: "source excerpt".to_string(),
                weight: 0.8,
            }],
        )
        .unwrap();

    store
        .set_confidence(
            &monitor,
            &root,
            id,
            ConfidenceScore {
                value: 0.85,
                method: ConfidenceMethod::Heuristic,
            },
            vec![Alternative {
                description: "shorter draft".to_string(),
                score: 0.5,
                rejection_reason: "less complete".to_string(),
            }],
        )
        .unwrap();

    store
        .transition(&monitor, &root, id, ControlState::Executing)
        .unwrap();
    store
        .transition(&monitor, &root, id, ControlState::Completed)
        .unwrap();

    let record = store.get(id).unwrap();
    assert_eq!(record.reasoning_chain.len(), 1);
    assert_eq!(record.evidence.len(), 1);
    assert_eq!(record.confidence.unwrap().value, 0.85);
    assert_eq!(record.alternatives.len(), 1);
    assert_eq!(record.control_state, ControlState::Completed);
}

#[test]
fn an_action_never_closed_is_flagged_by_the_completeness_invariant() {
    let (monitor, root, store) = setup();
    let id = store
        .begin(&monitor, &root, 1, 7, 42, "document.draft", vec![], 1_000)
        .unwrap();
    store
        .transition(&monitor, &root, id, ControlState::Executing)
        .unwrap();
    // Crashed before reaching Completed/RolledBack.

    let incomplete = store.incomplete();
    assert_eq!(incomplete.len(), 1);
    assert_eq!(incomplete[0].id, id);
}

#[test]
fn a_completed_action_is_not_flagged_incomplete() {
    let (monitor, root, store) = setup();
    let id = store
        .begin(&monitor, &root, 1, 7, 42, "document.draft", vec![], 1_000)
        .unwrap();
    store
        .transition(&monitor, &root, id, ControlState::Completed)
        .unwrap();

    assert!(store.incomplete().is_empty());
}

#[test]
fn a_rolled_back_action_is_terminal_and_not_flagged_incomplete() {
    let (monitor, root, store) = setup();
    let id = store
        .begin(&monitor, &root, 1, 7, 42, "document.draft", vec![], 1_000)
        .unwrap();
    store
        .transition(&monitor, &root, id, ControlState::RolledBack)
        .unwrap();

    assert!(store.incomplete().is_empty());
}

#[test]
fn attaching_an_undo_ref_carries_the_upstream_risk_engines_decision_verbatim() {
    let (monitor, root, store) = setup();
    let id = store
        .begin(&monitor, &root, 1, 7, 42, "kg.delete_many", vec![], 1_000)
        .unwrap();
    store.attach_undo_ref(&monitor, &root, id, 99).unwrap();

    let record = store.get(id).unwrap();
    assert_eq!(record.undo_ref, Some(99));
}
