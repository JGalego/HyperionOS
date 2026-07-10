//! docs/29-database-schema.md's worked "vacation scenario" example: a photo
//! and a hotel booking never reference each other directly, both tied to a
//! trip object — traversal from the trip must reach both via the *backward*
//! direction (they point at the trip, not the reverse), proving the
//! bidirectional-union traversal in docs/29 §Algorithms.

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
fn vacation_scenario_two_hop_traversal_from_trip() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let trip = graph
        .put_node(
            &monitor,
            &token,
            None,
            "trip",
            None,
            json!({"name": "Hawaii"}),
        )
        .unwrap();
    let photo = graph
        .put_node(
            &monitor,
            &token,
            None,
            "photo",
            None,
            json!({"camera": "iPhone 16"}),
        )
        .unwrap();
    let hotel = graph
        .put_node(
            &monitor,
            &token,
            None,
            "hotel_booking",
            None,
            json!({"confirmation": "HYP-88213"}),
        )
        .unwrap();
    let loc = graph
        .put_node(
            &monitor,
            &token,
            None,
            "location",
            None,
            json!({"name": "Maui"}),
        )
        .unwrap();

    graph
        .link(
            &monitor,
            &token,
            photo,
            "photographed_at",
            loc,
            1.0,
            EdgeOrigin::Inferred,
            Some(0.94),
            "inferred:gps-cluster",
            None,
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
            Some(0.88),
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

    // Neither photo nor hotel reference each other; both point AT trip, so
    // reaching them from trip requires the backward arm of the traversal.
    let subgraph = graph.traverse(&monitor, &token, trip, None, 1).unwrap();
    let reached: std::collections::HashSet<_> = subgraph.nodes.iter().map(|(id, _)| *id).collect();
    assert!(reached.contains(&trip));
    assert!(reached.contains(&photo));
    assert!(reached.contains(&hotel));
    assert!(
        !reached.contains(&loc),
        "location is 2 hops from trip, not 1"
    );

    let subgraph_2hop = graph.traverse(&monitor, &token, trip, None, 2).unwrap();
    let reached_2: std::collections::HashSet<_> =
        subgraph_2hop.nodes.iter().map(|(id, _)| *id).collect();
    assert!(reached_2.contains(&loc), "location IS 2 hops from trip");
}

#[test]
fn traverse_unknown_node_is_not_found() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let result = graph.traverse(&monitor, &token, hyperion_storage::ObjectId(999), None, 1);
    assert!(matches!(
        result,
        Err(hyperion_knowledge_graph::GraphError::NotFound)
    ));
}

#[test]
fn index_survives_reopen_by_replaying_the_wal() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let path = dir.path().join("kg.jsonl");

    let (trip, photo) = {
        let graph = KnowledgeGraph::open(&path).unwrap();
        let trip = graph
            .put_node(&monitor, &token, None, "trip", None, json!({}))
            .unwrap();
        let photo = graph
            .put_node(&monitor, &token, None, "photo", None, json!({}))
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
                Some(0.8),
                "agent:trip-assembler",
                None,
            )
            .unwrap();
        (trip, photo)
    };

    let reopened = KnowledgeGraph::open(&path).unwrap();
    let subgraph = reopened.traverse(&monitor, &token, trip, None, 1).unwrap();
    let reached: std::collections::HashSet<_> = subgraph.nodes.iter().map(|(id, _)| *id).collect();
    assert!(reached.contains(&photo));
}
