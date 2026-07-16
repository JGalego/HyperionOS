//! docs/09 §8's "capability-checked at every hop, not merely at the query boundary" -- already
//! real for `query`/`traverse`/`dump`, now real for this crate's single-object accessors too:
//! `get`/`get_at_version`/`delete_node`/`unlink`/`explain` all excluded a tombstoned object but
//! never checked `owner` against the caller's own Trust Boundary, contradicting `traverse`'s own
//! doc comment claim that `get` already gave the same "never reveal existence of what you can't
//! see" shape it does. `put_node` had a related, more severe bug: updating an *existing* node
//! always overwrote its `owner` to the caller's own boundary, letting any caller with a live
//! WRITE-rights token silently steal a foreign-boundary node -- and, worse, use that theft to
//! bypass every other owner check this file proves.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{EdgeOrigin, ExplainRef, GraphError, KnowledgeGraph};
use serde_json::json;

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    hyperion_capability::CapabilityToken,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let boundary_1 = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let boundary_2 = monitor.mint_root(RightsMask::all(), TrustBoundaryId(2), None);
    (dir, monitor, boundary_1, boundary_2)
}

#[test]
fn get_never_returns_a_different_trust_boundarys_node() {
    let (dir, monitor, boundary_1, boundary_2) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let theirs = graph
        .put_node(&monitor, &boundary_1, None, "Note", None, json!({"x": 1}))
        .unwrap();

    assert!(matches!(
        graph.get(&monitor, &boundary_2, theirs),
        Err(GraphError::NotFound)
    ));
    assert!(graph.get(&monitor, &boundary_1, theirs).is_ok());
}

#[test]
fn get_at_version_never_returns_a_different_trust_boundarys_node() {
    let (dir, monitor, boundary_1, boundary_2) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let theirs = graph
        .put_node(&monitor, &boundary_1, None, "Note", None, json!({"x": 1}))
        .unwrap();
    let v = graph.current_version(theirs).unwrap();

    assert!(matches!(
        graph.get_at_version(&monitor, &boundary_2, theirs, v),
        Err(GraphError::NotFound)
    ));
    assert!(graph
        .get_at_version(&monitor, &boundary_1, theirs, v)
        .is_ok());
}

#[test]
fn delete_node_never_deletes_a_different_trust_boundarys_node_and_never_reveals_it_exists() {
    let (dir, monitor, boundary_1, boundary_2) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let theirs = graph
        .put_node(&monitor, &boundary_1, None, "Note", None, json!({"x": 1}))
        .unwrap();

    assert!(matches!(
        graph.delete_node(&monitor, &boundary_2, theirs),
        Err(GraphError::NotFound)
    ));
    // Never actually tombstoned -- the owner's own read still succeeds.
    assert!(graph.get(&monitor, &boundary_1, theirs).is_ok());
}

#[test]
fn unlink_never_deletes_a_different_trust_boundarys_edge() {
    let (dir, monitor, boundary_1, boundary_2) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let a = graph
        .put_node(&monitor, &boundary_1, None, "a", None, json!({}))
        .unwrap();
    let b = graph
        .put_node(&monitor, &boundary_1, None, "b", None, json!({}))
        .unwrap();
    let edge_id = match graph
        .link(
            &monitor,
            &boundary_1,
            a,
            "rel",
            b,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "test",
            None,
        )
        .unwrap()
    {
        hyperion_knowledge_graph::LinkOutcome::Created(id) => id,
        other => panic!("expected Created, got {other:?}"),
    };

    assert!(matches!(
        graph.unlink(&monitor, &boundary_2, edge_id),
        Err(GraphError::NotFound)
    ));
    // Never actually tombstoned -- a real subsequent traverse from the owner still finds it.
    let subgraph = graph.traverse(&monitor, &boundary_1, a, None, 1).unwrap();
    assert!(subgraph.edges.iter().any(|(id, _)| *id == edge_id));
}

#[test]
fn explain_never_leaks_a_different_trust_boundarys_node_or_edge_provenance() {
    let (dir, monitor, boundary_1, boundary_2) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let a = graph
        .put_node(&monitor, &boundary_1, None, "a", None, json!({}))
        .unwrap();
    let b = graph
        .put_node(&monitor, &boundary_1, None, "b", None, json!({}))
        .unwrap();
    let edge_id = match graph
        .link(
            &monitor,
            &boundary_1,
            a,
            "rel",
            b,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "test",
            None,
        )
        .unwrap()
    {
        hyperion_knowledge_graph::LinkOutcome::Created(id) => id,
        other => panic!("expected Created, got {other:?}"),
    };

    assert!(matches!(
        graph.explain(&monitor, &boundary_2, ExplainRef::Node(a)),
        Err(GraphError::NotFound)
    ));
    assert!(matches!(
        graph.explain(&monitor, &boundary_2, ExplainRef::Edge(edge_id)),
        Err(GraphError::NotFound)
    ));
    assert!(graph
        .explain(&monitor, &boundary_1, ExplainRef::Node(a))
        .is_ok());
    assert!(graph
        .explain(&monitor, &boundary_1, ExplainRef::Edge(edge_id))
        .is_ok());
}

#[test]
fn put_node_can_never_steal_a_different_trust_boundarys_node() {
    let (dir, monitor, boundary_1, boundary_2) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let theirs = graph
        .put_node(
            &monitor,
            &boundary_1,
            None,
            "Note",
            None,
            json!({"x": "original"}),
        )
        .unwrap();

    let result = graph.put_node(
        &monitor,
        &boundary_2,
        Some(theirs),
        "Note",
        None,
        json!({"x": "stolen"}),
    );
    assert!(
        matches!(result, Err(GraphError::NotFound)),
        "a caller must never be able to overwrite -- and thereby reassign ownership of -- a \
         node it doesn't own; got {result:?}"
    );

    // The original owner's own view is genuinely untouched.
    let still_theirs = graph.get(&monitor, &boundary_1, theirs).unwrap();
    assert_eq!(still_theirs.metadata, json!({"x": "original"}));
    assert!(
        graph.get(&monitor, &boundary_2, theirs).is_err(),
        "ownership must never have actually transferred"
    );
}

#[test]
fn prune_decayed_edges_only_ever_prunes_the_callers_own_edges() {
    let (dir, monitor, boundary_1, boundary_2) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let a = graph
        .put_node(&monitor, &boundary_1, None, "a", None, json!({}))
        .unwrap();
    let b = graph
        .put_node(&monitor, &boundary_1, None, "b", None, json!({}))
        .unwrap();
    graph
        .link(
            &monitor,
            &boundary_1,
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

    // boundary_2 sweeping must never error out over boundary_1's own decayed edge, nor prune it.
    let pruned = graph
        .prune_decayed_edges(&monitor, &boundary_2, 1.0, u64::MAX / 2)
        .unwrap();
    assert!(pruned.is_empty());
    let subgraph = graph.traverse(&monitor, &boundary_1, a, None, 1).unwrap();
    assert!(
        !subgraph.edges.is_empty(),
        "boundary_1's own edge must be untouched"
    );
}
