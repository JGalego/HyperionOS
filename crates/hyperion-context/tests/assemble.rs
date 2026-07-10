//! docs/06-context-engine.md's exit bar for Phase 2: a Context Bundle can be
//! assembled for a synthetic Intent, correctly ranked and bounded in size.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::{Budget, ContextEngine, InclusionMode, Scope};
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
fn assemble_ranks_anchor_and_traversal_neighbors_above_unrelated_objects() {
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
            json!({"name": "the API"}),
        )
        .unwrap();
    let issue = graph
        .put_node(
            &monitor,
            &token,
            None,
            "issue",
            None,
            json!({"title": "flaky test"}),
        )
        .unwrap();
    graph
        .link(
            &monitor,
            &token,
            issue,
            "part_of",
            repo,
            1.0,
            hyperion_knowledge_graph::EdgeOrigin::Explicit,
            None,
            "user_explicit",
            None,
        )
        .unwrap();
    let unrelated = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"title": "unrelated notes"}),
        )
        .unwrap();

    let scope = Scope {
        intent_id: "intent-1".to_string(),
        session_id: "session-1".to_string(),
        mentions: Vec::new(),
        anchors: vec![repo],
    };
    let bundle = engine
        .assemble(&monitor, &token, &scope, Budget::default())
        .unwrap();

    let ranked_ids: Vec<_> = bundle.entries.iter().map(|e| e.node_id).collect();
    assert!(ranked_ids.contains(&repo));
    assert!(ranked_ids.contains(&issue));
    let repo_score = bundle
        .entries
        .iter()
        .find(|e| e.node_id == repo)
        .unwrap()
        .relevance_score;
    let unrelated_score = bundle
        .entries
        .iter()
        .find(|e| e.node_id == unrelated)
        .map(|e| e.relevance_score)
        .unwrap_or(0.0);
    assert!(repo_score > unrelated_score);
}

#[test]
fn scope_intent_id_naming_a_real_intent_node_pulls_it_in_without_being_an_explicit_anchor() {
    let (dir, monitor, token) = setup();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let engine = ContextEngine::new(graph.clone());

    // Mirrors how `hyperion-intent` actually persists an Intent: a real
    // node, type "intent", in this same graph.
    let intent_node = graph
        .put_node(
            &monitor,
            &token,
            None,
            "intent",
            None,
            json!({"predicate": "launch_my_startup"}),
        )
        .unwrap();
    let unrelated = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"title": "unrelated notes"}),
        )
        .unwrap();

    let scope = Scope {
        intent_id: intent_node.0.to_string(),
        session_id: "session-1".to_string(),
        mentions: Vec::new(),
        // Deliberately no explicit anchors -- the Intent node must be
        // pulled in purely because `scope.intent_id` names it.
        anchors: Vec::new(),
    };
    let bundle = engine
        .assemble(&monitor, &token, &scope, Budget::default())
        .unwrap();

    let ids: Vec<_> = bundle.entries.iter().map(|e| e.node_id).collect();
    assert!(
        ids.contains(&intent_node),
        "the real Intent node scope.intent_id names must enter the candidate pool"
    );
    assert!(!ids.contains(&unrelated));
}

#[test]
fn an_intent_id_that_is_not_a_real_node_is_silently_ignored() {
    let (dir, monitor, token) = setup();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let engine = ContextEngine::new(graph.clone());

    let scope = Scope {
        intent_id: "not-a-number".to_string(),
        session_id: "session-1".to_string(),
        mentions: Vec::new(),
        anchors: Vec::new(),
    };
    let bundle = engine
        .assemble(&monitor, &token, &scope, Budget::default())
        .unwrap();
    assert!(bundle.entries.is_empty());
}

#[test]
fn assemble_never_exceeds_the_token_budget() {
    let (dir, monitor, token) = setup();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let engine = ContextEngine::new(graph.clone());

    let mut anchors = Vec::new();
    for i in 0..50 {
        let id = graph
            .put_node(
                &monitor,
                &token,
                None,
                "document",
                None,
                json!({"title": format!("document {i}"), "body": "x".repeat(2000)}),
            )
            .unwrap();
        anchors.push(id);
    }

    let scope = Scope {
        intent_id: "intent-1".to_string(),
        session_id: "session-1".to_string(),
        mentions: Vec::new(),
        anchors: anchors.clone(),
    };
    let tiny_budget = Budget {
        max_tokens: 300,
        max_entries_per_category: 100,
    };
    let bundle = engine
        .assemble(&monitor, &token, &scope, tiny_budget)
        .unwrap();

    assert!(
        bundle.entries.len() < anchors.len(),
        "budget must cut off before all 50 anchors fit"
    );
}

#[test]
fn assemble_excludes_objects_owned_by_a_different_trust_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let owner_token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let other_token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(2), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let engine = ContextEngine::new(graph.clone());

    let mine = graph
        .put_node(
            &monitor,
            &owner_token,
            None,
            "document",
            None,
            json!({"title": "mine"}),
        )
        .unwrap();
    let theirs = graph
        .put_node(
            &monitor,
            &other_token,
            None,
            "document",
            None,
            json!({"title": "theirs"}),
        )
        .unwrap();

    let scope = Scope {
        intent_id: "intent-1".to_string(),
        session_id: "session-1".to_string(),
        mentions: Vec::new(),
        anchors: vec![mine, theirs],
    };
    let bundle = engine
        .assemble(&monitor, &owner_token, &scope, Budget::default())
        .unwrap();

    let ids: Vec<_> = bundle.entries.iter().map(|e| e.node_id).collect();
    assert!(ids.contains(&mine));
    assert!(
        !ids.contains(&theirs),
        "cross-boundary object must never enter the pool"
    );
}

#[test]
fn small_high_relevance_entries_are_included_full_not_referenced() {
    let (dir, monitor, token) = setup();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let engine = ContextEngine::new(graph.clone());

    let anchor = graph
        .put_node(
            &monitor,
            &token,
            None,
            "ticket",
            None,
            json!({"title": "small ticket"}),
        )
        .unwrap();

    let scope = Scope {
        intent_id: "intent-1".to_string(),
        session_id: "session-1".to_string(),
        mentions: Vec::new(),
        anchors: vec![anchor],
    };
    let bundle = engine
        .assemble(&monitor, &token, &scope, Budget::default())
        .unwrap();
    let entry = bundle.entries.iter().find(|e| e.node_id == anchor).unwrap();
    assert_eq!(entry.inclusion_mode, InclusionMode::Full);
    assert_eq!(entry.content, json!({"title": "small ticket"}));
}
