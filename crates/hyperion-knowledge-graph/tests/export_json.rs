//! `KnowledgeGraph::export_json` -- this crate's own previously-unnamed "no real JSON graph
//! export API" gap, exercised here against a genuine put_node/link round trip.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{EdgeOrigin, KnowledgeGraph};
use serde_json::{json, Value};

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
fn an_empty_graph_exports_to_valid_json_with_empty_arrays() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let json = graph.export_json(&monitor, &token).unwrap();
    let parsed: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["nodes"].as_array().unwrap().len(), 0);
    assert_eq!(parsed["edges"].as_array().unwrap().len(), 0);
}

#[test]
fn a_real_node_and_edge_export_with_their_real_ids_and_fields() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let subject = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"title": "quarterly plan"}),
        )
        .unwrap();
    let target = graph
        .put_node(&monitor, &token, None, "document", None, json!({}))
        .unwrap();
    graph
        .link(
            &monitor,
            &token,
            subject,
            "relates_to",
            target,
            0.5,
            EdgeOrigin::Explicit,
            None,
            "user_explicit",
            None,
        )
        .unwrap();

    let json = graph.export_json(&monitor, &token).unwrap();
    let parsed: Value = serde_json::from_str(&json).unwrap();

    let nodes = parsed["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 2);
    let subject_export = nodes
        .iter()
        .find(|n| n["id"].as_u64().unwrap() == subject.0)
        .unwrap();
    assert_eq!(subject_export["object_type"], "document");
    assert_eq!(subject_export["metadata"]["title"], "quarterly plan");

    let edges = parsed["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0]["subject"], subject.0);
    assert_eq!(edges[0]["target"], target.0);
    assert_eq!(edges[0]["predicate"], "relates_to");
}

#[test]
fn a_tombstoned_node_is_never_exported() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let node = graph
        .put_node(&monitor, &token, None, "note", None, json!({}))
        .unwrap();
    graph.delete_node(&monitor, &token, node).unwrap();

    let json = graph.export_json(&monitor, &token).unwrap();
    let parsed: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["nodes"].as_array().unwrap().len(), 0);
}

#[test]
fn a_different_trust_boundarys_nodes_never_export() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let boundary_1 = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let boundary_2 = monitor.mint_root(RightsMask::all(), TrustBoundaryId(2), None);
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    graph
        .put_node(&monitor, &boundary_2, None, "document", None, json!({}))
        .unwrap();

    let json = graph.export_json(&monitor, &boundary_1).unwrap();
    let parsed: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["nodes"].as_array().unwrap().len(), 0);
}
