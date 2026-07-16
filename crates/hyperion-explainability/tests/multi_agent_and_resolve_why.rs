//! docs/18 §5's multi-agent composition DAG and `resolve_why`'s headline/
//! full depth tiering.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_explainability::{
    resolve_why, ConfidenceMethod, ConfidenceScore, ControlState, Depth, ExplanationStore,
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
fn resolve_why_finds_a_record_by_action_id_alone() {
    let (monitor, root, store) = setup();
    let id = store
        .begin(&monitor, &root, 42, 7, 1, "web.research", vec![], 1_000)
        .unwrap();
    store
        .set_confidence(
            &monitor,
            &root,
            id,
            ConfidenceScore {
                value: 0.6,
                method: ConfidenceMethod::Heuristic,
            },
            vec![],
        )
        .unwrap();

    let view = resolve_why(&store, &monitor, &root, 42, Depth::Headline)
        .unwrap()
        .unwrap();
    assert!(view.headline.contains("web.research"));
    assert!(view.headline.contains("60%"));
    assert!(
        view.full.is_none(),
        "headline depth must not leak the full record"
    );
}

#[test]
fn full_depth_includes_the_complete_record() {
    let (monitor, root, store) = setup();
    let id = store
        .begin(&monitor, &root, 42, 7, 1, "web.research", vec![], 1_000)
        .unwrap();
    let _ = id;

    let view = resolve_why(&store, &monitor, &root, 42, Depth::Full)
        .unwrap()
        .unwrap();
    assert!(view.full.is_some());
}

#[test]
fn an_unknown_action_id_resolves_to_nothing() {
    let (monitor, root, store) = setup();
    assert!(resolve_why(&store, &monitor, &root, 999, Depth::Headline)
        .unwrap()
        .is_none());
}

#[test]
fn a_multi_agent_answer_resolves_root_first_then_expands_to_contributing_parents() {
    let (monitor, root, store) = setup();
    let coordinator = store
        .begin(&monitor, &root, 1, 7, 1, "coordination.plan", vec![], 1_000)
        .unwrap();
    let worker = store
        .begin(&monitor, &root, 2, 7, 2, "document.draft", vec![], 1_005)
        .unwrap();
    store
        .link_parent(&monitor, &root, coordinator, worker)
        .unwrap();

    let root_view = resolve_why(&store, &monitor, &root, 1, Depth::Full)
        .unwrap()
        .unwrap();
    assert_eq!(root_view.parents.len(), 1);
    assert!(root_view.parents[0].headline.contains("document.draft"));

    let worker_record = store.get(&monitor, &root, worker).unwrap().unwrap();
    assert_eq!(worker_record.child_records, vec![coordinator]);
}

#[test]
fn trace_intent_returns_every_action_recorded_under_one_intent() {
    let (monitor, root, store) = setup();
    let a = store
        .begin(&monitor, &root, 1, 7, 1, "web.research", vec![], 1_000)
        .unwrap();
    let b = store
        .begin(&monitor, &root, 2, 7, 1, "document.draft", vec![], 1_010)
        .unwrap();
    let c = store
        .begin(&monitor, &root, 3, 99, 1, "unrelated.intent", vec![], 1_020)
        .unwrap();

    let trace = store.trace_intent(&monitor, &root, 7).unwrap();
    let ids: Vec<_> = trace.iter().map(|r| r.id).collect();
    assert!(ids.contains(&a) && ids.contains(&b) && !ids.contains(&c));
}

#[test]
fn transition_on_an_unknown_record_fails() {
    let (monitor, root, store) = setup();
    let result = store.transition(&monitor, &root, 999, ControlState::Completed);
    assert!(matches!(
        result,
        Err(hyperion_explainability::ExplainabilityError::NoSuchRecord)
    ));
}
