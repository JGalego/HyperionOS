//! docs/09 §5.1's real, durable-reference framing needs a real historical-version read, layered
//! directly on `hyperion-storage`'s own version chain (`current_version`/`get_at_version`) rather
//! than through this crate's own live index, which only ever holds the *current* value per node.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
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
fn get_at_version_reads_back_a_superseded_value_the_live_index_no_longer_has() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let node = graph
        .put_node(&monitor, &token, None, "Note", None, json!({"text": "v1"}))
        .unwrap();
    let v1 = graph
        .current_version(node)
        .expect("a just-written node has a real current version");

    graph
        .put_node(
            &monitor,
            &token,
            Some(node),
            "Note",
            None,
            json!({"text": "v2"}),
        )
        .unwrap();
    let v2 = graph
        .current_version(node)
        .expect("the updated node has a real, different current version");
    assert_ne!(v1, v2, "sanity: the update really did mint a new version");

    // The live index (what `get` reads) only ever holds the current value.
    assert_eq!(
        graph.get(&monitor, &token, node).unwrap().metadata["text"],
        json!("v2")
    );

    // But the historical read recovers the superseded value the live index no longer has.
    let historical = graph.get_at_version(&monitor, &token, node, v1).unwrap();
    assert_eq!(historical.metadata["text"], json!("v1"));

    let current_via_version = graph.get_at_version(&monitor, &token, node, v2).unwrap();
    assert_eq!(current_via_version.metadata["text"], json!("v2"));
}

#[test]
fn get_at_version_on_an_unknown_version_is_not_found() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let node = graph
        .put_node(&monitor, &token, None, "Note", None, json!({"text": "v1"}))
        .unwrap();

    let bogus_version = hyperion_knowledge_graph::VersionId(999_999);
    let result = graph.get_at_version(&monitor, &token, node, bogus_version);
    assert!(matches!(
        result,
        Err(hyperion_knowledge_graph::GraphError::NotFound)
    ));
}
