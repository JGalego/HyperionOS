//! `NodeRecord::display_label` -- the real, canonical "describe this node to a person" primitive,
//! exercised here against a genuine `KnowledgeGraph::put_node`/`get` round trip rather than a
//! hand-built `NodeRecord`, so a real caller's own metadata shape (whatever `put_node` actually
//! stores) is what's under test.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{render_capability_result, KnowledgeGraph};
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
fn a_real_utterance_node_round_trips_through_display_label() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let id = graph
        .put_node(
            &monitor,
            &token,
            None,
            "intent",
            None,
            json!({"raw_utterance": "plan a trip to Lisbon", "confidence": 0.92}),
        )
        .unwrap();

    let node = graph.get(&monitor, &token, id).unwrap();
    assert_eq!(
        node.display_label(),
        "you asked: \"plan a trip to Lisbon\" (92% confident)"
    );
}

#[test]
fn a_real_decomposed_child_task_has_no_utterance_and_falls_back_to_its_predicate() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let id = graph
        .put_node(
            &monitor,
            &token,
            None,
            "intent",
            None,
            json!({"raw_utterance": "", "predicate": "market_research"}),
        )
        .unwrap();

    let node = graph.get(&monitor, &token, id).unwrap();
    assert!(node.utterance_text().is_none());
    assert_eq!(node.display_label(), "a planned task: market_research");
}

#[test]
fn a_real_task_result_node_renders_via_render_capability_result() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let id = graph
        .put_node(
            &monitor,
            &token,
            None,
            "task_result",
            None,
            json!({"results": ["a draft market analysis"], "note": "AI-generated, not a live web search"}),
        )
        .unwrap();

    let node = graph.get(&monitor, &token, id).unwrap();
    assert_eq!(
        node.display_label(),
        "a result: a draft market analysis (AI-generated, not a live web search)"
    );
    // The same primitive is independently callable against a bare `serde_json::Value` too --
    // the shape a caller rendering an in-flight result that isn't a graph node yet needs.
    assert_eq!(
        render_capability_result(&node.metadata).unwrap(),
        "a draft market analysis (AI-generated, not a live web search)"
    );
}

#[test]
fn an_unrecognized_real_node_never_panics_and_names_its_own_type_honestly() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let id = graph
        .put_node(
            &monitor,
            &token,
            None,
            "sensor_reading",
            None,
            json!({"celsius": 21.5}),
        )
        .unwrap();

    let node = graph.get(&monitor, &token, id).unwrap();
    assert_eq!(
        node.display_label(),
        "a sensor_reading ({\"celsius\":21.5})"
    );
}
