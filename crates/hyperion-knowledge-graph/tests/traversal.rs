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
    let reached: std::collections::HashSet<_> =
        subgraph.nodes.iter().map(|(id, _, _)| *id).collect();
    assert!(reached.contains(&trip));
    assert!(reached.contains(&photo));
    assert!(reached.contains(&hotel));
    assert!(
        !reached.contains(&loc),
        "location is 2 hops from trip, not 1"
    );

    let subgraph_2hop = graph.traverse(&monitor, &token, trip, None, 2).unwrap();
    let reached_2: std::collections::HashSet<_> =
        subgraph_2hop.nodes.iter().map(|(id, _, _)| *id).collect();
    assert!(reached_2.contains(&loc), "location IS 2 hops from trip");

    let depth_of = |id| {
        subgraph_2hop
            .nodes
            .iter()
            .find(|(node_id, _, _)| *node_id == id)
            .map(|(_, _, depth)| *depth)
            .unwrap()
    };
    assert_eq!(depth_of(trip), 0);
    assert_eq!(depth_of(photo), 1);
    assert_eq!(depth_of(hotel), 1);
    assert_eq!(depth_of(loc), 2);
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
    let reached: std::collections::HashSet<_> =
        subgraph.nodes.iter().map(|(id, _, _)| *id).collect();
    assert!(reached.contains(&photo));
}

/// docs/09 §8's "capability-checked at every hop": traversal must never expand into a node
/// outside the caller's own Trust Boundary, even when a real edge connects it to the anchor.
#[test]
fn traversal_never_crosses_into_a_different_trust_boundarys_node() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let boundary_1 = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let boundary_2 = monitor.mint_root(RightsMask::all(), TrustBoundaryId(2), None);
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let trip = graph
        .put_node(
            &monitor,
            &boundary_1,
            None,
            "trip",
            None,
            json!({"name": "Hawaii"}),
        )
        .unwrap();
    // A different Trust Boundary's own node, linked to trip -- a real edge exists, but hotel
    // itself belongs to a boundary the caller below doesn't hold.
    let hotel = graph
        .put_node(
            &monitor,
            &boundary_2,
            None,
            "hotel_booking",
            None,
            json!({"confirmation": "HYP-88213"}),
        )
        .unwrap();
    graph
        .link(
            &monitor,
            &boundary_2,
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

    let subgraph = graph
        .traverse(&monitor, &boundary_1, trip, None, 1)
        .unwrap();
    let reached: std::collections::HashSet<_> =
        subgraph.nodes.iter().map(|(id, _, _)| *id).collect();
    assert!(reached.contains(&trip));
    assert!(
        !reached.contains(&hotel),
        "a real edge exists, but hotel belongs to a different Trust Boundary -- it must never \
         be reachable by boundary_1's own traversal"
    );

    // The edge itself must not be marked visited/returned either -- not just the node.
    assert!(
        subgraph.edges.is_empty(),
        "the only real edge here crosses into an unauthorized node and must be excluded too"
    );
}

#[test]
fn traversing_from_a_node_owned_by_a_different_trust_boundary_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let boundary_1 = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let boundary_2 = monitor.mint_root(RightsMask::all(), TrustBoundaryId(2), None);
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let trip = graph
        .put_node(
            &monitor,
            &boundary_2,
            None,
            "trip",
            None,
            json!({"name": "Hawaii"}),
        )
        .unwrap();

    let result = graph.traverse(&monitor, &boundary_1, trip, None, 1);
    assert!(
        matches!(result, Err(hyperion_knowledge_graph::GraphError::NotFound)),
        "a real node that exists but belongs to a different boundary must read as not-found, \
         the same 'never reveal existence of what you can't see' shape a single get() gives"
    );
}
