//! docs/09 §5.2's own previously-named decay gap: "neither kind of inferred edge decays yet
//! (weight is reset to a fixed value each pass, not accumulated or aged)." `effective_edge_weight`
//! is the real, on-demand recency-weighted decay this closes -- an `Inferred` edge's real weight
//! shrinks with real elapsed time since its last real confirmation; an `Explicit` edge never
//! decays at all.

use std::time::{SystemTime, UNIX_EPOCH};

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{effective_edge_weight, EdgeOrigin, KnowledgeGraph};

const THIRTY_DAYS_SECS: u64 = 30 * 24 * 3_600;

fn real_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    (dir, monitor, token)
}

#[test]
fn a_freshly_confirmed_inferred_edge_has_effective_weight_close_to_its_stored_weight() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let a = graph
        .put_node(&monitor, &token, None, "Note", None, serde_json::json!({}))
        .unwrap();
    let b = graph
        .put_node(&monitor, &token, None, "Note", None, serde_json::json!({}))
        .unwrap();
    graph
        .link(
            &monitor,
            &token,
            a,
            "co-occurs-with",
            b,
            1.0,
            EdgeOrigin::Inferred,
            None,
            "test",
            None,
        )
        .unwrap();

    let snapshot = graph.dump(&monitor, &token).unwrap();
    let (_, edge) = snapshot
        .edges
        .iter()
        .find(|(_, e)| e.subject == a && e.target == b)
        .unwrap();

    let effective = effective_edge_weight(edge, real_now());
    assert!(
        (effective - 1.0).abs() < 0.01,
        "a just-confirmed edge must be close to full strength, got {effective}"
    );
}

#[test]
fn an_inferred_edge_really_decays_with_real_elapsed_time() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let a = graph
        .put_node(&monitor, &token, None, "Note", None, serde_json::json!({}))
        .unwrap();
    let b = graph
        .put_node(&monitor, &token, None, "Note", None, serde_json::json!({}))
        .unwrap();
    graph
        .link(
            &monitor,
            &token,
            a,
            "co-occurs-with",
            b,
            1.0,
            EdgeOrigin::Inferred,
            None,
            "test",
            None,
        )
        .unwrap();

    let snapshot = graph.dump(&monitor, &token).unwrap();
    let (_, edge) = snapshot
        .edges
        .iter()
        .find(|(_, e)| e.subject == a && e.target == b)
        .unwrap();

    // Simulate 60 real days having passed since the last real confirmation -- twice the tau.
    let far_future = real_now() + 2 * THIRTY_DAYS_SECS;
    let decayed = effective_edge_weight(edge, far_future);
    assert!(
        decayed < 0.2,
        "an inferred edge unconfirmed for twice its tau must have decayed substantially, got \
         {decayed}"
    );
    assert!(decayed >= 0.0, "a decayed weight must never go negative");
}

#[test]
fn an_explicit_edge_never_decays_regardless_of_elapsed_time() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let a = graph
        .put_node(&monitor, &token, None, "Note", None, serde_json::json!({}))
        .unwrap();
    let b = graph
        .put_node(&monitor, &token, None, "Note", None, serde_json::json!({}))
        .unwrap();
    graph
        .link(
            &monitor,
            &token,
            a,
            "part-of",
            b,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "test",
            None,
        )
        .unwrap();

    let snapshot = graph.dump(&monitor, &token).unwrap();
    let (_, edge) = snapshot
        .edges
        .iter()
        .find(|(_, e)| e.subject == a && e.target == b)
        .unwrap();

    let far_future = real_now() + 10 * THIRTY_DAYS_SECS;
    assert_eq!(
        effective_edge_weight(edge, far_future),
        1.0,
        "an explicit edge is not a hypothesis and must never decay, per docs/09 §5.2"
    );
}

#[test]
fn reconfirming_an_inferred_edge_restores_full_strength() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let a = graph
        .put_node(&monitor, &token, None, "Note", None, serde_json::json!({}))
        .unwrap();
    let b = graph
        .put_node(&monitor, &token, None, "Note", None, serde_json::json!({}))
        .unwrap();
    graph
        .link(
            &monitor,
            &token,
            a,
            "co-occurs-with",
            b,
            1.0,
            EdgeOrigin::Inferred,
            None,
            "test",
            None,
        )
        .unwrap();

    let far_future = real_now() + 2 * THIRTY_DAYS_SECS;
    {
        let snapshot = graph.dump(&monitor, &token).unwrap();
        let (_, edge) = snapshot
            .edges
            .iter()
            .find(|(_, e)| e.subject == a && e.target == b)
            .unwrap();
        assert!(
            effective_edge_weight(edge, far_future) < 0.2,
            "must have decayed before reconfirmation"
        );
    }

    // A real reconfirmation -- the same real co-occurrence pass firing again -- refreshes
    // last_confirmed_at.
    graph
        .link(
            &monitor,
            &token,
            a,
            "co-occurs-with",
            b,
            1.0,
            EdgeOrigin::Inferred,
            None,
            "test",
            None,
        )
        .unwrap();

    let snapshot = graph.dump(&monitor, &token).unwrap();
    let (_, edge) = snapshot
        .edges
        .iter()
        .find(|(_, e)| e.subject == a && e.target == b)
        .unwrap();
    let effective = effective_edge_weight(edge, real_now());
    assert!(
        (effective - 1.0).abs() < 0.01,
        "a real reconfirmation must restore full strength, got {effective}"
    );
}
