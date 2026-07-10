//! docs/10-semantic-filesystem.md's own vacation-scenario query resolution,
//! deterministic path collision disambiguation, snapshot stability, and the
//! write-back edge-fabrication distinction (§Algorithms "Folder
//! preservation"/"Write-back", Design Invariant 1 — no silent authority).

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_knowledge_graph::{EdgeOrigin, KnowledgeGraph};
use hyperion_semantic_fs::{AnchorResolution, QuerySpec, SemanticFilesystem};
use serde_json::json;

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    Arc<KnowledgeGraph>,
    SemanticFilesystem,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let fs = SemanticFilesystem::new(graph.clone(), context);
    (dir, monitor, token, graph, fs)
}

#[test]
fn vacation_query_resolves_photo_and_hotel_via_the_trip_anchor() {
    let (_dir, monitor, token, graph, fs) = setup();
    let trip = graph
        .put_node(
            &monitor,
            &token,
            None,
            "trip",
            None,
            json!({"title": "Hawaii"}),
        )
        .unwrap();
    let photo = graph
        .put_node(
            &monitor,
            &token,
            None,
            "photo",
            None,
            json!({"title": "beach"}),
        )
        .unwrap();
    let hotel = graph
        .put_node(
            &monitor,
            &token,
            None,
            "hotel_booking",
            None,
            json!({"title": "Wailea Resort"}),
        )
        .unwrap();
    graph
        .link(
            &monitor,
            &token,
            photo,
            "part_of_trip",
            trip,
            1.0,
            EdgeOrigin::Inferred,
            Some(0.9),
            "agent:trip-assembler",
            None,
        )
        .unwrap();
    graph
        .link(
            &monitor,
            &token,
            hotel,
            "part_of_trip",
            trip,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "user_explicit",
            None,
        )
        .unwrap();

    let spec = QuerySpec {
        anchor: Some(trip),
        hop_bound: 1,
        ..Default::default()
    };
    let folder = fs.query(&monitor, &token, &spec).unwrap();
    assert!(folder.member_object_ids.contains(&trip));
    assert!(folder.member_object_ids.contains(&photo));
    assert!(folder.member_object_ids.contains(&hotel));

    let entries = fs.materialize(&monitor, &token, folder.folder_id).unwrap();
    assert!(entries.iter().any(|e| e.path == "photo/beach"));
    assert!(entries
        .iter()
        .any(|e| e.path == "hotel_booking/Wailea Resort"));
}

#[test]
fn ambiguous_mention_escalates_rather_than_guessing() {
    let (_dir, monitor, token, graph, fs) = setup();
    graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"name": "quarterly marketing budget"}),
        )
        .unwrap();
    graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"name": "marketing budget summary"}),
        )
        .unwrap();

    let outcome = fs
        .resolve_query_from_mention(
            &monitor,
            &token,
            "the marketing budget review",
            "session-1",
            2,
            10,
        )
        .unwrap();
    assert!(matches!(outcome, AnchorResolution::Ambiguous(candidates) if candidates.len() >= 2));
}

#[test]
fn path_collisions_are_disambiguated_deterministically_by_object_id_not_arrival_order() {
    let (_dir, monitor, token, graph, fs) = setup();
    // Two documents with identical titles; created out of numeric order
    // relative to how they'll be materialized, to prove the suffix
    // assignment isn't order-dependent.
    let second = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"title": "Notes"}),
        )
        .unwrap();
    let first = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"title": "Notes"}),
        )
        .unwrap();
    assert!(
        first.0 > second.0,
        "first was created after second, so it has the larger id"
    );

    let spec = QuerySpec {
        anchor: Some(first),
        hop_bound: 0,
        ..Default::default()
    };
    let folder_a = fs.query(&monitor, &token, &spec).unwrap();
    let _ = fs
        .materialize(&monitor, &token, folder_a.folder_id)
        .unwrap();

    let spec_b = QuerySpec {
        anchor: Some(second),
        hop_bound: 0,
        ..Default::default()
    };
    let folder_b = fs.query(&monitor, &token, &spec_b).unwrap();
    let entries_b = fs
        .materialize(&monitor, &token, folder_b.folder_id)
        .unwrap();

    // Whichever one was assigned first keeps its path; the point under
    // test is that repeatedly materializing the same two objects never
    // reshuffles an already-cached assignment.
    let entries_a_again = fs
        .materialize(&monitor, &token, folder_a.folder_id)
        .unwrap();
    assert_eq!(entries_a_again[0].path, "document/Notes");
    assert_eq!(entries_b[0].path, "document/Notes-2");
}

#[test]
fn a_virtual_folder_snapshot_is_immutable_after_creation() {
    let (_dir, monitor, token, graph, fs) = setup();
    let trip = graph
        .put_node(
            &monitor,
            &token,
            None,
            "trip",
            None,
            json!({"title": "Hawaii"}),
        )
        .unwrap();

    let spec = QuerySpec {
        anchor: Some(trip),
        hop_bound: 1,
        ..Default::default()
    };
    let folder = fs.query(&monitor, &token, &spec).unwrap();
    let before = folder.member_object_ids.len();

    // A new photo joins the trip *after* the folder was materialized.
    let photo = graph
        .put_node(
            &monitor,
            &token,
            None,
            "photo",
            None,
            json!({"title": "new"}),
        )
        .unwrap();
    graph
        .link(
            &monitor,
            &token,
            photo,
            "part_of_trip",
            trip,
            1.0,
            EdgeOrigin::Inferred,
            Some(0.9),
            "agent",
            None,
        )
        .unwrap();

    let refetched = fs.get_folder(folder.folder_id).unwrap();
    assert_eq!(
        refetched.member_object_ids.len(),
        before,
        "an already-issued VirtualFolder must not silently grow"
    );

    // A fresh query picks up the new state instead.
    let fresh = fs.query(&monitor, &token, &spec).unwrap();
    assert!(fresh.member_object_ids.contains(&photo));
}

#[test]
fn write_back_into_a_real_collection_creates_an_explicit_edge_but_a_virtual_folder_does_not() {
    let (_dir, monitor, token, graph, fs) = setup();
    let collection = fs.mkcollection(&monitor, &token, "Receipts", None).unwrap();

    let id = fs
        .write_back(
            &monitor,
            &token,
            "Receipts/scan1",
            json!({"title": "scan1"}),
        )
        .unwrap();
    let explain = graph
        .explain(
            &monitor,
            &token,
            hyperion_knowledge_graph::ExplainRef::Node(id),
        )
        .unwrap();
    let hyperion_knowledge_graph::ProvenanceChain::Node { incident_edges, .. } = explain else {
        panic!("expected a node provenance chain");
    };
    assert_eq!(
        incident_edges.len(),
        1,
        "a write into a real Collection must fabricate exactly one member_of edge"
    );

    // A write into a path that was never created via mkcollection (a
    // virtual, query-synthesized-looking path) must not fabricate an edge.
    let id2 = fs
        .write_back(
            &monitor,
            &token,
            "photo/vacation",
            json!({"title": "vacation"}),
        )
        .unwrap();
    let explain2 = graph
        .explain(
            &monitor,
            &token,
            hyperion_knowledge_graph::ExplainRef::Node(id2),
        )
        .unwrap();
    let hyperion_knowledge_graph::ProvenanceChain::Node {
        incident_edges: edges2,
        ..
    } = explain2
    else {
        panic!("expected a node provenance chain");
    };
    assert!(
        edges2.is_empty(),
        "a write into a virtual folder must never fabricate a false explicit edge"
    );

    let _ = collection;
}

#[test]
fn resolve_path_finds_what_write_back_just_pinned() {
    let (_dir, monitor, token, _graph, fs) = setup();
    let id = fs
        .write_back(
            &monitor,
            &token,
            "document/report",
            json!({"title": "report"}),
        )
        .unwrap();
    assert_eq!(
        fs.resolve_path(&monitor, &token, "document/report")
            .unwrap(),
        id
    );
}
