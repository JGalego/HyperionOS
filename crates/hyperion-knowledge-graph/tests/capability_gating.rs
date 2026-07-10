//! Mirrors hyperion-storage's capability_gating.rs — every public entry
//! point here re-checks rights against the live `CapabilityMonitor`, never a
//! cached liveness result.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{EdgeOrigin, ExplainRef, GraphError, GraphQuery, KnowledgeGraph};
use serde_json::json;

#[test]
fn put_node_requires_write_rights() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();

    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let result = graph.put_node(&monitor, &read_only, None, "note", None, json!({}));
    assert!(matches!(result, Err(GraphError::Unauthorized)));
}

#[test]
fn get_requires_read_rights() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let node = graph
        .put_node(&monitor, &root, None, "note", None, json!({}))
        .unwrap();

    let write_only = monitor
        .cap_derive(&root, RightsMask::WRITE, None, TrustBoundaryId(2))
        .unwrap();
    assert!(matches!(
        graph.get(&monitor, &write_only, node),
        Err(GraphError::Unauthorized)
    ));
}

#[test]
fn link_query_traverse_and_explain_all_require_matching_rights() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let a = graph
        .put_node(&monitor, &root, None, "a", None, json!({}))
        .unwrap();
    let b = graph
        .put_node(&monitor, &root, None, "b", None, json!({}))
        .unwrap();

    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();
    assert!(matches!(
        graph.link(
            &monitor,
            &read_only,
            a,
            "rel",
            b,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "t",
            None
        ),
        Err(GraphError::Unauthorized)
    ));

    let outcome = graph
        .link(
            &monitor,
            &root,
            a,
            "rel",
            b,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "t",
            None,
        )
        .unwrap();
    let edge_id = match outcome {
        hyperion_knowledge_graph::LinkOutcome::Created(id) => id,
        other => panic!("expected Created, got {other:?}"),
    };

    let write_only = monitor
        .cap_derive(&root, RightsMask::WRITE, None, TrustBoundaryId(3))
        .unwrap();
    assert!(matches!(
        graph.query(&monitor, &write_only, &GraphQuery::default()),
        Err(GraphError::Unauthorized)
    ));
    assert!(matches!(
        graph.traverse(&monitor, &write_only, a, None, 1),
        Err(GraphError::Unauthorized)
    ));
    assert!(matches!(
        graph.explain(&monitor, &write_only, ExplainRef::Node(a)),
        Err(GraphError::Unauthorized)
    ));
    assert!(matches!(
        graph.unlink(&monitor, &read_only, edge_id),
        Err(GraphError::Unauthorized)
    ));
}

#[test]
fn revoking_a_token_blocks_further_access_re_checked_live() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let delegate = monitor
        .cap_derive(
            &root,
            RightsMask::READ | RightsMask::WRITE,
            None,
            TrustBoundaryId(2),
        )
        .unwrap();

    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let node = graph
        .put_node(&monitor, &delegate, None, "note", None, json!({}))
        .unwrap();
    assert!(graph.get(&monitor, &delegate, node).is_ok());

    monitor.cap_revoke(&delegate);

    assert!(matches!(
        graph.get(&monitor, &delegate, node),
        Err(GraphError::Unauthorized)
    ));
}
