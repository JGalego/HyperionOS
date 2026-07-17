//! `KnowledgeGraph::open_encrypted` closes `hyperion-storage`'s own named "no encryption at
//! rest" gap for a real Knowledge Graph caller: the underlying WAL is real, per-record
//! AEAD-encrypted, keyed by a device identity's own derived key -- no new passphrase, and a
//! different device key recovers an empty graph, never wrong or garbage data.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_knowledge_graph::KnowledgeGraph;
use serde_json::json;

#[test]
fn an_encrypted_knowledge_graph_never_writes_plaintext_metadata_to_disk() {
    let dir = tempfile::tempdir().unwrap();
    let kg_path = dir.path().join("graph.jsonl");
    let device_key = Keystore::ephemeral();

    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);

    let graph = KnowledgeGraph::open_encrypted(&kg_path, &device_key).unwrap();
    graph
        .put_node(
            &monitor,
            &token,
            None,
            "note",
            None,
            json!({"text": "a very secret real diary entry"}),
        )
        .unwrap();
    drop(graph);

    let raw = std::fs::read_to_string(&kg_path).unwrap();
    assert!(
        !raw.contains("very secret real diary entry"),
        "the real plaintext must never appear on disk once encryption at rest is enabled: {raw:?}"
    );
}

#[test]
fn an_encrypted_knowledge_graph_round_trips_through_a_real_reopen_with_the_same_device_key() {
    let dir = tempfile::tempdir().unwrap();
    let kg_path = dir.path().join("graph.jsonl");
    let device_key = Keystore::ephemeral();

    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);

    let node = {
        let graph = KnowledgeGraph::open_encrypted(&kg_path, &device_key).unwrap();
        graph
            .put_node(
                &monitor,
                &token,
                None,
                "note",
                None,
                json!({"text": "trip itinerary"}),
            )
            .unwrap()
    };

    let recovered = KnowledgeGraph::open_encrypted(&kg_path, &device_key)
        .expect("reopening with the same real device key must succeed");
    let record = recovered.get(&monitor, &token, node).unwrap();
    assert_eq!(record.metadata, json!({"text": "trip itinerary"}));
}

#[test]
fn opening_an_encrypted_graph_with_a_different_device_key_recovers_nothing_not_garbage() {
    let dir = tempfile::tempdir().unwrap();
    let kg_path = dir.path().join("graph.jsonl");
    let device_key_a = Keystore::ephemeral();
    let device_key_b = Keystore::ephemeral();

    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);

    let node = {
        let graph = KnowledgeGraph::open_encrypted(&kg_path, &device_key_a).unwrap();
        graph
            .put_node(
                &monitor,
                &token,
                None,
                "note",
                None,
                json!({"text": "trip itinerary"}),
            )
            .unwrap()
    };

    let wrong_key_graph = KnowledgeGraph::open_encrypted(&kg_path, &device_key_b)
        .expect("opening must still succeed -- the graph is just treated as empty");
    assert!(
        wrong_key_graph.get(&monitor, &token, node).is_err(),
        "a different device key must recover none of the real data, not garbage"
    );
}
