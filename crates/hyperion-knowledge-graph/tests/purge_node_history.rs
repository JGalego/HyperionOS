//! `hyperion-privacy`'s own previously-named "still not a byte-level deletion from the WAL's
//! history" gap: `KnowledgeGraph::delete_node` genuinely tombstones a node (gone from every
//! `get`/`query`/`traverse`/`dump`), but its past versions still sat, fully readable, in the
//! underlying WAL, reachable via `get_at_version`. `purge_node_history` closes that: it really
//! deletes every WAL record the node ever had, current head included.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{GraphError, KnowledgeGraph};
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
fn purge_node_history_removes_every_historical_version() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let node = graph
        .put_node(&monitor, &token, None, "document", None, json!({"v": 1}))
        .unwrap();
    let v1 = graph.current_version(node).unwrap();
    graph
        .put_node(
            &monitor,
            &token,
            Some(node),
            "document",
            None,
            json!({"v": 2}),
        )
        .unwrap();

    let purged = graph.purge_node_history(&monitor, &token, node).unwrap();
    assert_eq!(purged, 2, "both real historical versions must be counted");

    assert!(matches!(
        graph.get(&monitor, &token, node),
        Err(GraphError::NotFound)
    ));
    assert!(
        matches!(
            graph.get_at_version(&monitor, &token, node, v1),
            Err(GraphError::NotFound)
        ),
        "a purged node's own past version must no longer be reachable at all, unlike a plain \
         tombstone which leaves history readable via get_at_version"
    );
}

#[test]
fn purge_node_history_works_on_an_already_tombstoned_node() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let node = graph
        .put_node(&monitor, &token, None, "document", None, json!({}))
        .unwrap();
    graph.delete_node(&monitor, &token, node).unwrap();

    let purged = graph.purge_node_history(&monitor, &token, node).unwrap();
    assert_eq!(purged, 2, "the create plus the tombstone write");
}

#[test]
fn purge_node_history_never_touches_a_different_trust_boundarys_node() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let mine = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let theirs = monitor.mint_root(RightsMask::all(), TrustBoundaryId(2), None);
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let node = graph
        .put_node(&monitor, &theirs, None, "document", None, json!({}))
        .unwrap();

    let result = graph.purge_node_history(&monitor, &mine, node);
    assert!(matches!(result, Err(GraphError::NotFound)));
    // The real owner must still be able to read it -- nothing was actually purged.
    assert!(graph.get(&monitor, &theirs, node).is_ok());
}

#[test]
fn purge_node_history_on_an_unknown_node_is_not_found() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let result = graph.purge_node_history(&monitor, &token, hyperion_storage::ObjectId(9999));
    assert!(matches!(result, Err(GraphError::NotFound)));
}

#[test]
fn purge_node_history_never_disturbs_a_different_nodes_own_history() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let target = graph
        .put_node(&monitor, &token, None, "document", None, json!({"v": 1}))
        .unwrap();
    graph
        .put_node(
            &monitor,
            &token,
            Some(target),
            "document",
            None,
            json!({"v": 2}),
        )
        .unwrap();

    let survivor = graph
        .put_node(&monitor, &token, None, "document", None, json!({"v": 1}))
        .unwrap();
    let survivor_v1 = graph.current_version(survivor).unwrap();
    graph
        .put_node(
            &monitor,
            &token,
            Some(survivor),
            "document",
            None,
            json!({"v": 2}),
        )
        .unwrap();

    graph.purge_node_history(&monitor, &token, target).unwrap();

    assert!(graph.get(&monitor, &token, survivor).is_ok());
    assert_eq!(
        graph
            .get_at_version(&monitor, &token, survivor, survivor_v1)
            .unwrap()
            .metadata,
        json!({"v": 1}),
        "purging a different node must never collapse the survivor's own real history"
    );
}
