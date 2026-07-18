//! This crate's own previously-named "`ProvenanceRecord`/trust-scoring for Knowledge Graph
//! poisoning (T4)" gap, closed for the node-schema half: [`NodeOrigin`]/`corroboration_count`
//! are docs/17 §6's real `ProvenanceRecord.origin_type`/`corroboration_count`, and
//! [`KnowledgeGraph::corroborate_node`] is the real "independently reconfirmed" event. See
//! `hyperion-security::kg_trust_score` for the real consuming trust-scoring function these feed.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{GraphError, KnowledgeGraph, NodeOrigin};
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
fn a_plain_put_node_defaults_to_the_least_trusted_origin_and_zero_corroboration() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let id = graph
        .put_node(&monitor, &token, None, "note", None, json!({}))
        .unwrap();
    let node = graph.get(&monitor, &token, id).unwrap();
    assert_eq!(
        node.origin,
        NodeOrigin::IngestedExternal,
        "a caller with no real provenance to supply must never default to a more-trusted value"
    );
    assert_eq!(node.corroboration_count, 0);
}

#[test]
fn put_node_with_provenance_records_a_real_origin() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let id = graph
        .put_node_with_provenance(
            &monitor,
            &token,
            None,
            "note",
            None,
            json!({}),
            0,
            NodeOrigin::UserAuthored,
            hyperion_knowledge_graph::TenantId::default(),
        )
        .unwrap();
    let node = graph.get(&monitor, &token, id).unwrap();
    assert_eq!(node.origin, NodeOrigin::UserAuthored);
}

#[test]
fn updating_a_node_overwrites_origin_to_reflect_the_current_version() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let id = graph
        .put_node_with_provenance(
            &monitor,
            &token,
            None,
            "note",
            None,
            json!({"v": 1}),
            0,
            NodeOrigin::IngestedExternal,
            hyperion_knowledge_graph::TenantId::default(),
        )
        .unwrap();
    graph
        .put_node_with_provenance(
            &monitor,
            &token,
            Some(id),
            "note",
            None,
            json!({"v": 2}),
            0,
            NodeOrigin::UserAuthored,
            hyperion_knowledge_graph::TenantId::default(),
        )
        .unwrap();

    let node = graph.get(&monitor, &token, id).unwrap();
    assert_eq!(
        node.origin,
        NodeOrigin::UserAuthored,
        "origin reflects whoever authored the CURRENT version, unlike owner which is preserved"
    );
}

#[test]
fn corroborate_node_increments_the_real_count_and_a_content_update_never_resets_it() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let id = graph
        .put_node(&monitor, &token, None, "note", None, json!({}))
        .unwrap();
    assert_eq!(graph.corroborate_node(&monitor, &token, id).unwrap(), 1);
    assert_eq!(graph.corroborate_node(&monitor, &token, id).unwrap(), 2);
    assert_eq!(
        graph.get(&monitor, &token, id).unwrap().corroboration_count,
        2
    );

    // A plain content update is not, by itself, a re-confirmation by anyone else.
    graph
        .put_node(&monitor, &token, Some(id), "note", None, json!({"v": 2}))
        .unwrap();
    assert_eq!(
        graph.get(&monitor, &token, id).unwrap().corroboration_count,
        2,
        "an ordinary content update must never reset or bump corroboration_count on its own"
    );
}

#[test]
fn corroborate_node_never_touches_a_different_trust_boundarys_node() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let mine = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let theirs = monitor.mint_root(RightsMask::all(), TrustBoundaryId(2), None);
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let id = graph
        .put_node(&monitor, &theirs, None, "note", None, json!({}))
        .unwrap();

    let result = graph.corroborate_node(&monitor, &mine, id);
    assert!(matches!(result, Err(GraphError::NotFound)));
}

#[test]
fn corroborate_node_on_an_unknown_id_is_not_found() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let result = graph.corroborate_node(&monitor, &token, hyperion_storage::ObjectId(9999));
    assert!(matches!(result, Err(GraphError::NotFound)));
}
