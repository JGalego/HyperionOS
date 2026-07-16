//! docs/28 §"Garbage collection / compaction"'s own named gap for this crate: "inferred edges
//! below a confidence threshold... are pruned... explicit edges... are never auto-pruned."
//! `KnowledgeGraph::prune_decayed_edges` is the real sweep: a non-tombstoned `Inferred` edge
//! whose real `effective_edge_weight` has fallen below a caller-chosen threshold is tombstoned
//! for real via the existing `unlink`; an `Explicit` edge is never even considered.

use std::time::{SystemTime, UNIX_EPOCH};

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{EdgeOrigin, KnowledgeGraph};

const THIRTY_DAYS_SECS: u64 = 30 * 24 * 3_600;
const THRESHOLD: f32 = 0.2;

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

fn edge_id_for(
    graph: &KnowledgeGraph,
    monitor: &CapabilityMonitor,
    token: &hyperion_capability::CapabilityToken,
    subject: hyperion_knowledge_graph::NodeId,
    target: hyperion_knowledge_graph::NodeId,
) -> hyperion_knowledge_graph::EdgeId {
    graph
        .dump(monitor, token)
        .unwrap()
        .edges
        .into_iter()
        .find(|(_, e)| e.subject == subject && e.target == target)
        .map(|(id, _)| id)
        .unwrap()
}

#[test]
fn a_deeply_decayed_inferred_edge_is_tombstoned_by_a_prune_pass() {
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
    let id = edge_id_for(&graph, &monitor, &token, a, b);

    let far_future = real_now() + 2 * THIRTY_DAYS_SECS;
    let pruned = graph
        .prune_decayed_edges(&monitor, &token, THRESHOLD, far_future)
        .unwrap();
    assert_eq!(pruned, vec![id]);

    let snapshot = graph.dump(&monitor, &token).unwrap();
    assert!(
        !snapshot.edges.iter().any(|(eid, _)| *eid == id),
        "a pruned edge must never be surfaced by dump again"
    );
}

#[test]
fn a_freshly_confirmed_inferred_edge_survives_a_prune_pass() {
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
    let id = edge_id_for(&graph, &monitor, &token, a, b);

    let pruned = graph
        .prune_decayed_edges(&monitor, &token, THRESHOLD, real_now())
        .unwrap();
    assert!(pruned.is_empty());

    let snapshot = graph.dump(&monitor, &token).unwrap();
    assert!(snapshot.edges.iter().any(|(eid, _)| *eid == id));
}

#[test]
fn an_explicit_edge_is_never_pruned_regardless_of_age_or_threshold() {
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
    let id = edge_id_for(&graph, &monitor, &token, a, b);

    let far_future = real_now() + 100 * THIRTY_DAYS_SECS;
    let pruned = graph
        .prune_decayed_edges(&monitor, &token, 1.0, far_future)
        .unwrap();
    assert!(
        pruned.is_empty(),
        "an explicit edge must never be pruned, even with an extreme threshold and far-future now"
    );

    let snapshot = graph.dump(&monitor, &token).unwrap();
    assert!(snapshot.edges.iter().any(|(eid, _)| *eid == id));
}

#[test]
fn a_second_prune_pass_over_an_already_pruned_edge_evicts_nothing_new() {
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
    let first = graph
        .prune_decayed_edges(&monitor, &token, THRESHOLD, far_future)
        .unwrap();
    assert_eq!(first.len(), 1);

    let second = graph
        .prune_decayed_edges(&monitor, &token, THRESHOLD, far_future)
        .unwrap();
    assert!(
        second.is_empty(),
        "an already-tombstoned edge must never be re-pruned"
    );
}

#[test]
fn pruning_is_durable_across_a_real_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let wal_path = dir.path().join("kg.jsonl");
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);

    let id = {
        let graph = KnowledgeGraph::open(&wal_path).unwrap();
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
        let id = edge_id_for(&graph, &monitor, &token, a, b);

        let far_future = real_now() + 2 * THIRTY_DAYS_SECS;
        graph
            .prune_decayed_edges(&monitor, &token, THRESHOLD, far_future)
            .unwrap();
        id
    };

    let recovered = KnowledgeGraph::open(&wal_path).unwrap();
    let snapshot = recovered.dump(&monitor, &token).unwrap();
    assert!(
        !snapshot.edges.iter().any(|(eid, _)| *eid == id),
        "a real prune must survive a fresh reopen, not just live in the in-memory index"
    );
}
