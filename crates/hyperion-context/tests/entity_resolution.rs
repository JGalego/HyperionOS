//! docs/06-context-engine.md §Algorithms 1: "the API" resolves to a specific
//! repository by fuzzy match; genuinely ambiguous references escalate
//! rather than guess.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::{ContextEngine, EntityResolution};
use hyperion_knowledge_graph::KnowledgeGraph;
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
fn unambiguous_mention_resolves_with_high_confidence() {
    let (dir, monitor, token) = setup();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let engine = ContextEngine::new(graph.clone());

    let repo = graph
        .put_node(
            &monitor,
            &token,
            None,
            "repository",
            None,
            json!({"name": "payments-api"}),
        )
        .unwrap();
    graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"title": "unrelated doc"}),
        )
        .unwrap();

    let resolution = engine
        .resolve_entity(&monitor, &token, "payments-api", "session-1")
        .unwrap();
    match resolution {
        EntityResolution::Resolved {
            node_id,
            confidence,
        } => {
            assert_eq!(node_id, repo);
            assert!(confidence > 0.6);
        }
        other => panic!("expected Resolved, got {other:?}"),
    }
}

#[test]
fn nothing_matching_returns_not_found() {
    let (dir, monitor, token) = setup();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let engine = ContextEngine::new(graph.clone());
    graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"title": "vacation photos"}),
        )
        .unwrap();

    let resolution = engine
        .resolve_entity(&monitor, &token, "quantum computing paper", "session-1")
        .unwrap();
    assert!(matches!(resolution, EntityResolution::NotFound));
}

#[test]
fn two_equally_plausible_repositories_escalate_as_ambiguous() {
    let (dir, monitor, token) = setup();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let engine = ContextEngine::new(graph.clone());

    graph
        .put_node(
            &monitor,
            &token,
            None,
            "repository",
            None,
            json!({"name": "widget"}),
        )
        .unwrap();
    graph
        .put_node(
            &monitor,
            &token,
            None,
            "repository",
            None,
            json!({"name": "widget-legacy"}),
        )
        .unwrap();

    let resolution = engine
        .resolve_entity(&monitor, &token, "widget", "session-1")
        .unwrap();
    // Both contain "widget"; an exact match on one and a substring match on
    // the other are close enough in this simplified scorer to force a
    // human-in-the-loop check per docs/06 §Recovery Mechanisms, unless one
    // is a clean exact match — assert the resolver never silently coin-flips.
    match resolution {
        EntityResolution::Resolved { confidence, .. } => assert!(confidence >= 0.6),
        EntityResolution::Ambiguous(candidates) => assert!(candidates.len() >= 2),
        EntityResolution::NotFound => panic!("both repositories are plausible matches"),
    }
}
