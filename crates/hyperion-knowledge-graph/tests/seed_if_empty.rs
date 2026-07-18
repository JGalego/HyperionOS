//! `KnowledgeGraph::seed_if_empty` -- the real "first run" half of this crate's own
//! previously-unnamed "no pre-population/seed API" gap: seeds a starter dataset only when the
//! caller's own Trust Boundary has recorded nothing yet, never re-seeding or duplicating an
//! already-populated graph.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;

const SAMPLE: &str = r#"{
    "nodes": [
        {"id": 1, "object_type": "welcome", "metadata": {"title": "hello"}, "owner": 0, "created_at": 0, "updated_at": 0}
    ],
    "edges": []
}"#;

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
fn a_genuinely_empty_graph_gets_seeded() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let report = graph
        .seed_if_empty(&monitor, &token, SAMPLE)
        .unwrap()
        .expect("a fresh graph must actually be seeded");
    assert_eq!(report.nodes_created, 1);
}

#[test]
fn seeding_is_never_repeated_once_something_is_recorded() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    graph.seed_if_empty(&monitor, &token, SAMPLE).unwrap();
    let second = graph.seed_if_empty(&monitor, &token, SAMPLE).unwrap();
    assert!(
        second.is_none(),
        "a second call must be a real no-op, never a duplicate seed"
    );

    let hits = graph
        .query(
            &monitor,
            &token,
            &hyperion_knowledge_graph::GraphQuery::default(),
        )
        .unwrap();
    assert_eq!(
        hits.len(),
        1,
        "the sample dataset must never be seeded twice"
    );
}

#[test]
fn a_graph_that_already_has_real_content_before_seeding_is_never_touched() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    graph
        .put_node(
            &monitor,
            &token,
            None,
            "intent",
            None,
            serde_json::json!({"raw_utterance": "already said something"}),
        )
        .unwrap();

    let result = graph.seed_if_empty(&monitor, &token, SAMPLE).unwrap();
    assert!(result.is_none());

    let hits = graph
        .query(
            &monitor,
            &token,
            &hyperion_knowledge_graph::GraphQuery::default(),
        )
        .unwrap();
    assert_eq!(
        hits.len(),
        1,
        "only the caller's own real content, no seed added"
    );
}

#[test]
fn one_trust_boundarys_content_never_blocks_another_boundarys_own_first_seed() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let boundary_1 = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let boundary_2 = monitor.mint_root(RightsMask::all(), TrustBoundaryId(2), None);
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    graph
        .put_node(
            &monitor,
            &boundary_1,
            None,
            "document",
            None,
            serde_json::json!({}),
        )
        .unwrap();

    let report = graph
        .seed_if_empty(&monitor, &boundary_2, SAMPLE)
        .unwrap()
        .expect("boundary_2 has recorded nothing of its own yet, so it should still be seeded");
    assert_eq!(report.nodes_created, 1);
}
