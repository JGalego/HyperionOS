//! `KnowledgeGraph::dump` -- the whole visible graph in one call, built for a caller that wants
//! to inspect or diff its structure (e.g. `hyperion-console`'s own `/graph` meta-command).

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{EdgeOrigin, KnowledgeGraph};
use serde_json::json;

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
fn dump_of_a_fresh_graph_is_empty() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let snapshot = graph.dump(&monitor, &token).unwrap();
    assert!(snapshot.nodes.is_empty());
    assert!(snapshot.edges.is_empty());
}

#[test]
fn dump_includes_every_real_node_and_edge() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let a = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"title": "a"}),
        )
        .unwrap();
    let b = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"title": "b"}),
        )
        .unwrap();
    graph
        .link(
            &monitor,
            &token,
            a,
            "related-to",
            b,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "test",
            None,
        )
        .unwrap();

    let snapshot = graph.dump(&monitor, &token).unwrap();
    let node_ids: Vec<_> = snapshot.nodes.iter().map(|(id, _)| *id).collect();
    assert_eq!(
        node_ids,
        vec![a, b],
        "expected both real nodes, in id order"
    );
    assert_eq!(snapshot.edges.len(), 1, "expected the one real edge");
    assert_eq!(snapshot.edges[0].1.subject, a);
    assert_eq!(snapshot.edges[0].1.target, b);
}

/// The whole reason `dump` sorts by id rather than trusting the underlying `HashMap`'s own
/// iteration order: two dumps of an unchanged graph must be identical, so a caller can diff two
/// dumps (e.g. before/after a scenario) without ordering noise producing a false-positive change.
#[test]
fn two_dumps_of_an_unchanged_graph_are_identical() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    for i in 0..8 {
        graph
            .put_node(
                &monitor,
                &token,
                None,
                "document",
                None,
                json!({"title": format!("doc {i}")}),
            )
            .unwrap();
    }

    let first = graph.dump(&monitor, &token).unwrap();
    let second = graph.dump(&monitor, &token).unwrap();

    let first_ids: Vec<_> = first.nodes.iter().map(|(id, _)| *id).collect();
    let second_ids: Vec<_> = second.nodes.iter().map(|(id, _)| *id).collect();
    assert_eq!(first_ids, second_ids);
    assert!(
        first_ids.windows(2).all(|w| w[0] < w[1]),
        "expected strictly ascending ids, got: {first_ids:?}"
    );
}

/// A tombstoned edge (deleted, per docs/09 §5.4's CRDT semantics) must disappear from a dump the
/// same way it already disappears from `query`/`traverse` -- so a caller diffing a before/after
/// pair of dumps sees a real deletion as the edge vanishing, not as stale ghost content.
#[test]
fn a_tombstoned_edge_is_omitted() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let a = graph
        .put_node(&monitor, &token, None, "document", None, json!({}))
        .unwrap();
    let b = graph
        .put_node(&monitor, &token, None, "document", None, json!({}))
        .unwrap();
    let outcome = graph
        .link(
            &monitor,
            &token,
            a,
            "related-to",
            b,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "test",
            None,
        )
        .unwrap();
    let hyperion_knowledge_graph::LinkOutcome::Created(edge_id) = outcome else {
        panic!("expected a freshly created edge, got: {outcome:?}");
    };
    assert_eq!(graph.dump(&monitor, &token).unwrap().edges.len(), 1);

    graph.unlink(&monitor, &token, edge_id).unwrap();
    assert!(
        graph.dump(&monitor, &token).unwrap().edges.is_empty(),
        "a tombstoned edge must not appear in a dump"
    );
}

/// docs/09 §8's "capability-checked at every hop," applied to a full dump: a node or edge
/// belonging to a different Trust Boundary must never appear, even though it's a real record in
/// the same underlying graph.
#[test]
fn dump_never_includes_a_different_trust_boundarys_nodes_or_edges() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let boundary_1 = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let boundary_2 = monitor.mint_root(RightsMask::all(), TrustBoundaryId(2), None);
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let mine = graph
        .put_node(&monitor, &boundary_1, None, "document", None, json!({}))
        .unwrap();
    let theirs = graph
        .put_node(&monitor, &boundary_2, None, "document", None, json!({}))
        .unwrap();
    graph
        .link(
            &monitor,
            &boundary_2,
            theirs,
            "related-to",
            theirs,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "test",
            None,
        )
        .unwrap();

    let snapshot = graph.dump(&monitor, &boundary_1).unwrap();
    let node_ids: Vec<_> = snapshot.nodes.iter().map(|(id, _)| *id).collect();
    assert_eq!(node_ids, vec![mine]);
    assert!(
        snapshot.edges.is_empty(),
        "the other boundary's own edge must never appear"
    );
}
