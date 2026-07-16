//! This crate's own previously-named "no node-delete operation (only edges tombstone)" gap:
//! `KnowledgeGraph::delete_node` tombstones a node exactly the way `unlink` already tombstones an
//! edge, per docs/09 §10's own "deletions are tombstones... undoable within a retention window"
//! precedent -- now real for nodes too. `get`/`query`/`traverse`/`dump` all treat a tombstoned
//! node as genuinely gone.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{EdgeOrigin, GraphError, GraphQuery, KnowledgeGraph};
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
fn a_deleted_node_is_really_not_found() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let node = graph
        .put_node(&monitor, &token, None, "document", None, json!({}))
        .unwrap();
    assert!(graph.get(&monitor, &token, node).is_ok());

    graph.delete_node(&monitor, &token, node).unwrap();

    let result = graph.get(&monitor, &token, node);
    assert!(
        matches!(result, Err(GraphError::NotFound)),
        "got: {result:?}"
    );
}

#[test]
fn deleting_an_unknown_node_is_a_real_not_found_error() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let result = graph.delete_node(&monitor, &token, hyperion_storage::ObjectId(999));
    assert!(
        matches!(result, Err(GraphError::NotFound)),
        "got: {result:?}"
    );
}

#[test]
fn deleting_an_already_deleted_node_is_a_real_no_op() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let node = graph
        .put_node(&monitor, &token, None, "document", None, json!({}))
        .unwrap();

    graph.delete_node(&monitor, &token, node).unwrap();
    // A second delete on the same, already-tombstoned node must succeed, not error.
    graph.delete_node(&monitor, &token, node).unwrap();
}

#[test]
fn a_deleted_node_is_excluded_from_query_but_a_live_sibling_is_not() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let deleted = graph
        .put_node(&monitor, &token, None, "document", None, json!({}))
        .unwrap();
    let live = graph
        .put_node(&monitor, &token, None, "document", None, json!({}))
        .unwrap();
    graph.delete_node(&monitor, &token, deleted).unwrap();

    let hits = graph
        .query(&monitor, &token, &GraphQuery::default())
        .unwrap();
    let ids: Vec<_> = hits.iter().map(|h| h.node_id).collect();
    assert!(!ids.contains(&deleted), "deleted node leaked into query");
    assert!(ids.contains(&live), "live sibling wrongly excluded");
}

#[test]
fn a_deleted_node_is_excluded_from_dump() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let deleted = graph
        .put_node(&monitor, &token, None, "document", None, json!({}))
        .unwrap();
    let live = graph
        .put_node(&monitor, &token, None, "document", None, json!({}))
        .unwrap();
    graph.delete_node(&monitor, &token, deleted).unwrap();

    let snapshot = graph.dump(&monitor, &token).unwrap();
    let ids: Vec<_> = snapshot.nodes.iter().map(|(id, _)| *id).collect();
    assert!(!ids.contains(&deleted), "deleted node leaked into dump");
    assert!(ids.contains(&live), "live sibling wrongly excluded");
}

#[test]
fn traverse_cannot_start_from_a_deleted_node() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let node = graph
        .put_node(&monitor, &token, None, "document", None, json!({}))
        .unwrap();
    graph.delete_node(&monitor, &token, node).unwrap();

    let result = graph.traverse(&monitor, &token, node, None, 1);
    assert!(
        matches!(result, Err(GraphError::NotFound)),
        "a deleted start node must behave exactly like an unknown one, got: {result:?}"
    );
}

#[test]
fn traverse_never_expands_into_a_deleted_neighbor() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let anchor = graph
        .put_node(&monitor, &token, None, "document", None, json!({}))
        .unwrap();
    let deleted_neighbor = graph
        .put_node(&monitor, &token, None, "document", None, json!({}))
        .unwrap();
    graph
        .link(
            &monitor,
            &token,
            anchor,
            "rel",
            deleted_neighbor,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "test",
            None,
        )
        .unwrap();
    graph
        .delete_node(&monitor, &token, deleted_neighbor)
        .unwrap();

    let subgraph = graph.traverse(&monitor, &token, anchor, None, 1).unwrap();
    let ids: Vec<_> = subgraph.nodes.iter().map(|(id, _, _)| *id).collect();
    assert!(
        !ids.contains(&deleted_neighbor),
        "traversal must never expand into a deleted node, got: {subgraph:?}"
    );
}

#[test]
fn a_tombstone_survives_a_real_wal_replay() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let path = dir.path().join("kg.jsonl");

    let node = {
        let graph = KnowledgeGraph::open(&path).unwrap();
        let node = graph
            .put_node(&monitor, &token, None, "document", None, json!({}))
            .unwrap();
        graph.delete_node(&monitor, &token, node).unwrap();
        node
    };

    // A fresh handle over the same real WAL file -- the tombstone must replay, not just live in
    // the first handle's in-memory index.
    let reopened = KnowledgeGraph::open(&path).unwrap();
    let result = reopened.get(&monitor, &token, node);
    assert!(
        matches!(result, Err(GraphError::NotFound)),
        "a tombstoned node must stay tombstoned after a real WAL replay, got: {result:?}"
    );
}

#[test]
fn a_plain_put_node_update_never_resurrects_a_deleted_node() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let node = graph
        .put_node(&monitor, &token, None, "document", None, json!({}))
        .unwrap();
    graph.delete_node(&monitor, &token, node).unwrap();

    // An ordinary update to the same node id -- not a call to delete_node -- must not silently
    // revive it, the same "an insert never silently resurrects a deliberate deletion" invariant
    // `link` already enforces for edges.
    graph
        .put_node(
            &monitor,
            &token,
            Some(node),
            "document",
            None,
            json!({"updated": true}),
        )
        .unwrap();

    let result = graph.get(&monitor, &token, node);
    assert!(
        matches!(result, Err(GraphError::NotFound)),
        "a plain put_node update must not resurrect a tombstoned node, got: {result:?}"
    );
}
