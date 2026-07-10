//! docs/17 T4: Knowledge Graph / Semantic Filesystem poisoning — every
//! write is capability-checked and records its authoring Trust Boundary.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{GraphError, KnowledgeGraph};

#[test]
fn t4_an_unauthorized_writer_cannot_plant_a_node() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());

    let result = graph.put_node(
        &monitor,
        &read_only,
        None,
        "Organization",
        None,
        serde_json::json!({"name": "Fake Corp"}),
    );
    assert!(matches!(result, Err(GraphError::Unauthorized)));
}

#[test]
fn t4_every_planted_node_records_its_authoring_trust_boundary_for_later_audit() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(77), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());

    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Organization",
            None,
            serde_json::json!({"name": "Acme"}),
        )
        .unwrap();
    let record = graph.get(&monitor, &root, node).unwrap();

    assert_eq!(record.owner, 77, "a node's owner must trace to the real authoring Trust Boundary, not a caller-supplied claim");
}
