//! docs/09-knowledge-graph.md §7's hybrid query: type filter ∩ vector
//! similarity ∩ temporal window ∩ edge constraint.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{EdgeConstraint, EdgeOrigin, GraphQuery, KnowledgeGraph};
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
fn query_ranks_by_cosine_similarity_and_respects_type_filter() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let close = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            Some(vec![1.0, 0.0, 0.0]),
            json!({"title": "quantum computing paper"}),
        )
        .unwrap();
    let far = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            Some(vec![0.0, 1.0, 0.0]),
            json!({"title": "gardening tips"}),
        )
        .unwrap();
    let wrong_type = graph
        .put_node(
            &monitor,
            &token,
            None,
            "photo",
            Some(vec![1.0, 0.0, 0.0]),
            json!({"title": "a photo, coincidentally embedded near the query"}),
        )
        .unwrap();

    let query = GraphQuery {
        type_filter: Some(vec!["document".to_string()]),
        embedding_query: Some(vec![1.0, 0.0, 0.0]),
        limit: 10,
        ..Default::default()
    };
    let hits = graph.query(&monitor, &token, &query).unwrap();

    let ids: Vec<_> = hits.iter().map(|h| h.node_id).collect();
    assert!(
        !ids.contains(&wrong_type),
        "type filter must exclude non-document nodes"
    );
    assert_eq!(ids[0], close, "closest embedding must rank first");
    assert!(
        ids.contains(&far),
        "far document is still a candidate, just lower-ranked"
    );
    assert!(hits[0].score > hits[1].score);
}

#[test]
fn query_respects_time_range() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let node = graph
        .put_node(&monitor, &token, None, "note", None, json!({}))
        .unwrap();
    let created_at = graph.get(&monitor, &token, node).unwrap().created_at;

    let in_range = GraphQuery {
        time_range: Some((created_at, created_at)),
        limit: 0,
        ..Default::default()
    };
    assert_eq!(graph.query(&monitor, &token, &in_range).unwrap().len(), 1);

    let out_of_range = GraphQuery {
        time_range: Some((created_at + 1, created_at + 1000)),
        limit: 0,
        ..Default::default()
    };
    assert_eq!(
        graph.query(&monitor, &token, &out_of_range).unwrap().len(),
        0
    );
}

#[test]
fn query_edge_constraint_matches_either_direction() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let principal = graph
        .put_node(&monitor, &token, None, "person", None, json!({}))
        .unwrap();
    let paper = graph
        .put_node(&monitor, &token, None, "research_paper", None, json!({}))
        .unwrap();
    let unread_paper = graph
        .put_node(&monitor, &token, None, "research_paper", None, json!({}))
        .unwrap();

    graph
        .link(
            &monitor,
            &token,
            principal,
            "read-by",
            paper,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "user_explicit",
            None,
        )
        .unwrap();

    let query = GraphQuery {
        type_filter: Some(vec!["research_paper".to_string()]),
        edge_constraint: Some(EdgeConstraint {
            predicate: "read-by".to_string(),
            node: principal,
        }),
        limit: 0,
        ..Default::default()
    };
    let hits = graph.query(&monitor, &token, &query).unwrap();
    let ids: Vec<_> = hits.iter().map(|h| h.node_id).collect();
    assert_eq!(ids, vec![paper]);
    assert!(!ids.contains(&unread_paper));
}

/// docs/09 §8's "capability-checked at every hop": a query must never return a candidate
/// belonging to a different Trust Boundary, even one that would otherwise match every filter.
#[test]
fn query_never_returns_a_different_trust_boundarys_object() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let boundary_1 = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let boundary_2 = monitor.mint_root(RightsMask::all(), TrustBoundaryId(2), None);
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let mine = graph
        .put_node(
            &monitor,
            &boundary_1,
            None,
            "document",
            None,
            json!({"title": "mine"}),
        )
        .unwrap();
    let theirs = graph
        .put_node(
            &monitor,
            &boundary_2,
            None,
            "document",
            None,
            json!({"title": "theirs"}),
        )
        .unwrap();

    let query = GraphQuery {
        type_filter: Some(vec!["document".to_string()]),
        ..Default::default()
    };
    let hits = graph.query(&monitor, &boundary_1, &query).unwrap();
    let ids: Vec<_> = hits.iter().map(|h| h.node_id).collect();
    assert!(ids.contains(&mine));
    assert!(
        !ids.contains(&theirs),
        "a real, matching object exists, but it belongs to a different Trust Boundary"
    );
}
