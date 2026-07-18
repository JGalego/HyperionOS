//! `KnowledgeGraph::explain_ranking` -- docs/09 §7's own "graph.explain can show the cosine
//! similarity... and why it fell inside the fuzzy six-month window," answered for real: why one
//! node did (or didn't) rank for a given query, after the fact.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{
    EdgeConstraint, EdgeOrigin, GraphError, GraphQuery, KnowledgeGraph,
};
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
fn a_query_with_no_filters_reports_similarity_alone_and_always_would_be_included() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let node = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            Some(vec![1.0, 0.0, 0.0]),
            json!({}),
        )
        .unwrap();

    let query = GraphQuery {
        embedding_query: Some(vec![1.0, 0.0, 0.0]),
        ..Default::default()
    };
    let rationale = graph
        .explain_ranking(&monitor, &token, node, &query)
        .unwrap();

    assert!((rationale.similarity - 1.0).abs() < f32::EPSILON);
    assert_eq!(rationale.type_filter_matched, None);
    assert_eq!(rationale.within_time_range, None);
    assert_eq!(rationale.edge_constraint_satisfied, None);
    assert!(rationale.would_be_included);
}

#[test]
fn a_node_of_the_wrong_type_reports_why_it_would_be_excluded() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let photo = graph
        .put_node(&monitor, &token, None, "photo", None, json!({}))
        .unwrap();

    let query = GraphQuery {
        type_filter: Some(vec!["document".to_string()]),
        ..Default::default()
    };
    let rationale = graph
        .explain_ranking(&monitor, &token, photo, &query)
        .unwrap();

    assert_eq!(rationale.type_filter_matched, Some(false));
    assert!(!rationale.would_be_included);
}

#[test]
fn a_node_outside_the_time_range_reports_why_it_would_be_excluded() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let node = graph
        .put_node(&monitor, &token, None, "note", None, json!({}))
        .unwrap();
    let created_at = graph.get(&monitor, &token, node).unwrap().created_at;

    let query = GraphQuery {
        time_range: Some((created_at + 1, created_at + 1000)),
        ..Default::default()
    };
    let rationale = graph
        .explain_ranking(&monitor, &token, node, &query)
        .unwrap();

    assert_eq!(rationale.within_time_range, Some(false));
    assert!(!rationale.would_be_included);
}

#[test]
fn a_node_satisfying_an_edge_constraint_reports_it() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let principal = graph
        .put_node(&monitor, &token, None, "person", None, json!({}))
        .unwrap();
    let paper = graph
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
        edge_constraint: Some(EdgeConstraint {
            predicate: "read-by".to_string(),
            node: principal,
        }),
        ..Default::default()
    };
    let rationale = graph
        .explain_ranking(&monitor, &token, paper, &query)
        .unwrap();

    assert_eq!(rationale.edge_constraint_satisfied, Some(true));
    assert!(rationale.would_be_included);
}

#[test]
fn explain_ranking_agrees_with_query_on_every_hit_it_actually_returns() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let close = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            Some(vec![1.0, 0.0, 0.0]),
            json!({}),
        )
        .unwrap();
    let far = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            Some(vec![0.0, 1.0, 0.0]),
            json!({}),
        )
        .unwrap();

    let query = GraphQuery {
        type_filter: Some(vec!["document".to_string()]),
        embedding_query: Some(vec![1.0, 0.0, 0.0]),
        limit: 10,
        ..Default::default()
    };
    let hits = graph.query(&monitor, &token, &query).unwrap();

    for hit in &hits {
        let rationale = graph
            .explain_ranking(&monitor, &token, hit.node_id, &query)
            .unwrap();
        assert!(rationale.would_be_included);
        assert!((rationale.similarity - hit.score).abs() < f32::EPSILON);
    }
    assert!(hits.iter().any(|h| h.node_id == close));
    assert!(hits.iter().any(|h| h.node_id == far));
}

#[test]
fn an_unknown_node_is_a_real_not_found_error() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let query = GraphQuery::default();
    let result = graph.explain_ranking(&monitor, &token, hyperion_storage::ObjectId(9_999), &query);
    assert!(matches!(result, Err(GraphError::NotFound)));
}

#[test]
fn a_different_trust_boundarys_node_is_not_found_never_leaked() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let boundary_1 = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let boundary_2 = monitor.mint_root(RightsMask::all(), TrustBoundaryId(2), None);
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let theirs = graph
        .put_node(&monitor, &boundary_2, None, "document", None, json!({}))
        .unwrap();

    let query = GraphQuery::default();
    let result = graph.explain_ranking(&monitor, &boundary_1, theirs, &query);
    assert!(matches!(result, Err(GraphError::NotFound)));
}
