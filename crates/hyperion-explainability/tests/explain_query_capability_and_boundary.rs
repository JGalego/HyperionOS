//! docs/18 §8's own "access to an `explain.query` result is gated by the same capability grant
//! that gated the underlying data -- a user... cannot use the explanation channel as a side door
//! to read data they were never granted access to," and §13's explicit call for a test proving
//! `explain.query` never returns detail the caller lacks a capability grant for. Every read here
//! (`get`/`trace_intent`/`incomplete`/`calibration_score`/`resolve_why`) previously took no
//! `monitor`/`token` at all -- this is the missing test.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_explainability::{
    resolve_why, ConfidenceMethod, ConfidenceScore, ControlState, Depth, ExplainabilityError,
    ExplanationStore,
};

fn setup() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    hyperion_capability::CapabilityToken,
    ExplanationStore,
) {
    let mut monitor = CapabilityMonitor::new();
    let boundary_1 = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let boundary_2 = monitor.mint_root(RightsMask::all(), TrustBoundaryId(2), None);
    (monitor, boundary_1, boundary_2, ExplanationStore::new())
}

#[test]
fn get_requires_read_rights() {
    let (mut monitor, boundary_1, _boundary_2, store) = setup();
    let id = store
        .begin(
            &monitor,
            &boundary_1,
            1,
            7,
            1,
            "web.research",
            vec![],
            1_000,
        )
        .unwrap();
    let write_only = monitor
        .cap_derive(&boundary_1, RightsMask::WRITE, None, TrustBoundaryId(1))
        .unwrap();

    assert!(matches!(
        store.get(&monitor, &write_only, id),
        Err(ExplainabilityError::Unauthorized)
    ));
}

#[test]
fn get_never_returns_a_different_trust_boundarys_record() {
    let (monitor, boundary_1, boundary_2, store) = setup();
    let id = store
        .begin(
            &monitor,
            &boundary_1,
            1,
            7,
            1,
            "web.research",
            vec![],
            1_000,
        )
        .unwrap();

    assert!(store.get(&monitor, &boundary_2, id).unwrap().is_none());
    assert!(store.get(&monitor, &boundary_1, id).unwrap().is_some());
}

#[test]
fn trace_intent_never_returns_a_different_trust_boundarys_record() {
    let (monitor, boundary_1, boundary_2, store) = setup();
    store
        .begin(
            &monitor,
            &boundary_1,
            1,
            42,
            1,
            "web.research",
            vec![],
            1_000,
        )
        .unwrap();

    assert!(store
        .trace_intent(&monitor, &boundary_2, 42)
        .unwrap()
        .is_empty());
    assert_eq!(
        store.trace_intent(&monitor, &boundary_1, 42).unwrap().len(),
        1
    );
}

#[test]
fn incomplete_never_returns_a_different_trust_boundarys_record() {
    let (monitor, boundary_1, boundary_2, store) = setup();
    store
        .begin(
            &monitor,
            &boundary_1,
            1,
            7,
            1,
            "web.research",
            vec![],
            1_000,
        )
        .unwrap();
    // Never transitioned to a terminal state -- eligible for `incomplete` if visible at all.

    assert!(store.incomplete(&monitor, &boundary_2).unwrap().is_empty());
    assert_eq!(store.incomplete(&monitor, &boundary_1).unwrap().len(), 1);
}

#[test]
fn calibration_score_never_scores_over_a_different_trust_boundarys_records() {
    let (monitor, boundary_1, boundary_2, store) = setup();
    let id = store
        .begin(
            &monitor,
            &boundary_1,
            1,
            7,
            42,
            "document.draft",
            vec![],
            1_000,
        )
        .unwrap();
    store
        .set_confidence(
            &monitor,
            &boundary_1,
            id,
            ConfidenceScore {
                value: 0.9,
                method: ConfidenceMethod::Heuristic,
            },
            vec![],
        )
        .unwrap();
    store
        .transition(&monitor, &boundary_1, id, ControlState::Completed)
        .unwrap();

    assert!(store
        .calibration_score(&monitor, &boundary_2, 42, "document.draft")
        .unwrap()
        .is_none());
    assert!(store
        .calibration_score(&monitor, &boundary_1, 42, "document.draft")
        .unwrap()
        .is_some());
}

#[test]
fn resolve_why_never_resolves_a_different_trust_boundarys_record() {
    let (monitor, boundary_1, boundary_2, store) = setup();
    store
        .begin(
            &monitor,
            &boundary_1,
            1,
            7,
            1,
            "web.research",
            vec![],
            1_000,
        )
        .unwrap();

    assert!(
        resolve_why(&store, &monitor, &boundary_2, 1, Depth::Headline)
            .unwrap()
            .is_none()
    );
    assert!(
        resolve_why(&store, &monitor, &boundary_1, 1, Depth::Headline)
            .unwrap()
            .is_some()
    );
}

#[test]
fn resolve_why_full_depth_never_expands_into_a_different_trust_boundarys_parent() {
    let (monitor, boundary_1, boundary_2, store) = setup();
    let coordinator = store
        .begin(
            &monitor,
            &boundary_1,
            1,
            7,
            1,
            "coordination.plan",
            vec![],
            1_000,
        )
        .unwrap();
    // A parent record opened under a genuinely different Trust Boundary.
    let worker = store
        .begin(
            &monitor,
            &boundary_2,
            2,
            7,
            2,
            "document.draft",
            vec![],
            1_005,
        )
        .unwrap();
    store
        .link_parent(&monitor, &boundary_1, coordinator, worker)
        .unwrap();

    let view = resolve_why(&store, &monitor, &boundary_1, 1, Depth::Full)
        .unwrap()
        .unwrap();
    assert!(
        view.parents.is_empty(),
        "a parent record outside the caller's own Trust Boundary must never be expanded into, \
         got: {:?}",
        view.parents
    );
}
