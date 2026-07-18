//! Real ambient/continuous Knowledge Graph replication across devices -- neither
//! `hyperion-federation` nor `hyperion-storage` owned this before (each crate's own doc comment
//! pointed at the other). `merge_snapshot` is the real "apply a remote node/edge into my own
//! local graph" primitive this workspace had nowhere; `serve_kg_snapshots`/
//! `publish_snapshot_over_socket` really move a `GraphSnapshot` between two independent devices
//! over a real, encrypted+signed `TcpStream`; `KgAntiEntropyHeartbeat` keeps doing this on a real
//! wall-clock interval with no caller ever triggering a sync by hand.

use std::sync::Arc;
use std::time::Duration;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_federation::{
    merge_snapshot, publish_snapshot_over_socket, serve_kg_snapshots, FederationHub,
    KgAntiEntropyHeartbeat, KgTranslation,
};
use hyperion_knowledge_graph::KnowledgeGraph;

fn monitor_and_token() -> (CapabilityMonitor, hyperion_capability::CapabilityToken) {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    (monitor, token)
}

fn open_graph() -> (tempfile::TempDir, KnowledgeGraph) {
    let dir = tempfile::tempdir().unwrap();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    (dir, graph)
}

#[test]
fn merge_snapshot_creates_real_local_nodes_under_translated_ids() {
    let (monitor, token) = monitor_and_token();
    let (_dir_a, source) = open_graph();
    let (_dir_b, dest) = open_graph();
    let translation = KgTranslation::new();

    // Gives `dest`'s own id counter a head start so a coincidental match with the remote's raw
    // id (both graphs otherwise mint their very first node as the same low id) can never be
    // mistaken for a real "id reused directly" bug below.
    dest.put_node(&monitor, &token, None, "decoy", None, serde_json::json!({}))
        .unwrap();

    let remote_id = source
        .put_node(
            &monitor,
            &token,
            None,
            "note",
            None,
            serde_json::json!({"text": "from device A"}),
        )
        .unwrap();
    let snapshot = source.dump(&monitor, &token).unwrap();
    assert_eq!(snapshot.nodes.len(), 1);

    let report = merge_snapshot(&dest, &monitor, &token, &translation, &snapshot);
    assert_eq!(report.nodes_created, 1);
    assert_eq!(report.nodes_updated, 0);

    // The remote's own raw id must never be reused directly -- two independent graphs mint from
    // independent counters, so the same raw id would otherwise name the wrong local object.
    let dest_snapshot = dest.dump(&monitor, &token).unwrap();
    assert_eq!(
        dest_snapshot.nodes.len(),
        2,
        "the decoy plus the one real merged node"
    );
    let (local_id, record) = dest_snapshot
        .nodes
        .iter()
        .find(|(_, r)| r.object_type == "note")
        .expect("the merged node must really be present");
    assert_eq!(
        record.metadata,
        serde_json::json!({"text": "from device A"})
    );
    assert_ne!(
        local_id, &remote_id,
        "merging must mint a fresh local id, never reuse the remote's own raw id"
    );
}

#[test]
fn resyncing_the_same_source_updates_the_translated_copy_not_a_duplicate() {
    let (monitor, token) = monitor_and_token();
    let (_dir_a, source) = open_graph();
    let (_dir_b, dest) = open_graph();
    let translation = KgTranslation::new();

    let remote_id = source
        .put_node(
            &monitor,
            &token,
            None,
            "note",
            None,
            serde_json::json!({"version": 1}),
        )
        .unwrap();
    let first_snapshot = source.dump(&monitor, &token).unwrap();
    merge_snapshot(&dest, &monitor, &token, &translation, &first_snapshot);

    // The same remote node changes; a second sync must update the same translated local copy.
    source
        .put_node(
            &monitor,
            &token,
            Some(remote_id),
            "note",
            None,
            serde_json::json!({"version": 2}),
        )
        .unwrap();
    let second_snapshot = source.dump(&monitor, &token).unwrap();
    let report = merge_snapshot(&dest, &monitor, &token, &translation, &second_snapshot);
    assert_eq!(report.nodes_created, 0);
    assert_eq!(report.nodes_updated, 1);

    let dest_snapshot = dest.dump(&monitor, &token).unwrap();
    assert_eq!(
        dest_snapshot.nodes.len(),
        1,
        "resyncing must never create a second, duplicate local node for the same remote source"
    );
    assert_eq!(
        dest_snapshot.nodes[0].1.metadata,
        serde_json::json!({"version": 2})
    );
}

#[test]
fn edges_are_translated_to_local_ids_not_left_pointing_at_remote_ones() {
    let (monitor, token) = monitor_and_token();
    let (_dir_a, source) = open_graph();
    let (_dir_b, dest) = open_graph();
    let translation = KgTranslation::new();

    let a = source
        .put_node(&monitor, &token, None, "note", None, serde_json::json!({}))
        .unwrap();
    let b = source
        .put_node(&monitor, &token, None, "note", None, serde_json::json!({}))
        .unwrap();
    source
        .link(
            &monitor,
            &token,
            a,
            "relates-to",
            b,
            1.0,
            hyperion_knowledge_graph::EdgeOrigin::Explicit,
            None,
            "test",
            None,
        )
        .unwrap();

    let snapshot = source.dump(&monitor, &token).unwrap();
    let report = merge_snapshot(&dest, &monitor, &token, &translation, &snapshot);
    assert_eq!(report.edges_applied, 1);
    assert_eq!(report.edges_skipped_unresolved, 0);

    let dest_snapshot = dest.dump(&monitor, &token).unwrap();
    assert_eq!(dest_snapshot.edges.len(), 1);
    let (_, edge) = &dest_snapshot.edges[0];
    let local_ids: Vec<_> = dest_snapshot.nodes.iter().map(|(id, _)| *id).collect();
    assert!(
        local_ids.contains(&edge.subject) && local_ids.contains(&edge.target),
        "a merged edge must reference this graph's own real translated local node ids, not the \
         remote device's raw ones"
    );
}

#[test]
fn an_edge_referencing_a_never_translated_node_is_skipped_not_guessed_at() {
    let (monitor, token) = monitor_and_token();
    let (_dir, dest) = open_graph();
    let translation = KgTranslation::new();

    // A synthetic snapshot naming an edge whose endpoints were never part of any nodes list this
    // translation table has ever seen.
    let snapshot = hyperion_knowledge_graph::GraphSnapshot {
        nodes: Vec::new(),
        edges: vec![(
            hyperion_storage::ObjectId(1),
            hyperion_knowledge_graph::EdgeRecord {
                subject: hyperion_storage::ObjectId(100),
                predicate: "relates-to".to_string(),
                target: hyperion_storage::ObjectId(101),
                weight: 1.0,
                provenance: "test".to_string(),
                origin: hyperion_knowledge_graph::EdgeOrigin::Explicit,
                confidence: None,
                owner: 1,
                created_at: 0,
                last_confirmed_at: 0,
                tombstone: false,
                version: 1,
            },
        )],
    };

    let report = merge_snapshot(&dest, &monitor, &token, &translation, &snapshot);
    assert_eq!(report.edges_applied, 0);
    assert_eq!(report.edges_skipped_unresolved, 1);
    assert!(dest.dump(&monitor, &token).unwrap().edges.is_empty());
}

#[test]
fn a_graph_snapshot_really_travels_over_a_real_socket_and_merges_on_arrival() {
    let (monitor, token) = monitor_and_token();
    let monitor = Arc::new(monitor);
    let (_dir_a, source) = open_graph();
    let (_dir_b, dest) = open_graph();
    let dest = Arc::new(dest);

    source
        .put_node(
            &monitor,
            &token,
            None,
            "note",
            None,
            serde_json::json!({"text": "shipped over the real wire"}),
        )
        .unwrap();
    let snapshot = source.dump(&monitor, &token).unwrap();

    let hub_a = Arc::new(FederationHub::new());
    let hub_b = Arc::new(FederationHub::new());
    let shared_secret_a = hub_a.establish_shared_secret(&hub_b.x25519_public());
    let shared_secret_b = hub_b.establish_shared_secret(&hub_a.x25519_public());

    let translation = Arc::new(KgTranslation::new());
    let server = serve_kg_snapshots(
        Arc::clone(&hub_b),
        Arc::clone(&dest),
        Arc::clone(&monitor),
        token.clone(),
        Arc::clone(&translation),
        "127.0.0.1:0",
        hub_a.verifying_key(),
        shared_secret_b,
    )
    .expect("binding a real loopback TCP listener must succeed");

    publish_snapshot_over_socket(
        &hub_a,
        &server.local_addr().to_string(),
        &shared_secret_a,
        1,
        &snapshot,
    )
    .expect("connecting to the real, already-bound server must succeed");

    let mut merged = Vec::new();
    for _ in 0..200 {
        merged = dest.dump(&monitor, &token).unwrap().nodes;
        if !merged.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        merged.len(),
        1,
        "the real snapshot must arrive over the real socket and merge"
    );
    assert_eq!(
        merged[0].1.metadata,
        serde_json::json!({"text": "shipped over the real wire"})
    );

    server.stop();
}

#[test]
fn the_ambient_heartbeat_keeps_syncing_with_no_caller_ever_triggering_it_by_hand() {
    let (monitor, token) = monitor_and_token();
    let monitor = Arc::new(monitor);
    let (_dir_a, source) = open_graph();
    let source = Arc::new(source);
    let (_dir_b, dest) = open_graph();
    let dest = Arc::new(dest);

    let hub_a = Arc::new(FederationHub::new());
    let hub_b = Arc::new(FederationHub::new());
    let shared_secret_a = hub_a.establish_shared_secret(&hub_b.x25519_public());
    let shared_secret_b = hub_b.establish_shared_secret(&hub_a.x25519_public());

    let translation = Arc::new(KgTranslation::new());
    let server = serve_kg_snapshots(
        Arc::clone(&hub_b),
        Arc::clone(&dest),
        Arc::clone(&monitor),
        token.clone(),
        translation,
        "127.0.0.1:0",
        hub_a.verifying_key(),
        shared_secret_b,
    )
    .unwrap();

    let heartbeat = KgAntiEntropyHeartbeat::start(
        Arc::clone(&source),
        hub_a,
        Arc::clone(&monitor),
        token.clone(),
        server.local_addr().to_string(),
        shared_secret_a,
        1,
        Duration::from_millis(100),
    );

    // Only *after* starting the heartbeat does the source graph gain a real node -- proving the
    // next real tick (not a one-shot initial publish) is what carries it across, with no call to
    // `publish_snapshot_over_socket` anywhere in this test.
    source
        .put_node(
            &monitor,
            &token,
            None,
            "note",
            None,
            serde_json::json!({"text": "appeared after the heartbeat started"}),
        )
        .unwrap();

    let mut merged = Vec::new();
    for _ in 0..200 {
        merged = dest.dump(&monitor, &token).unwrap().nodes;
        if !merged.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        merged.len(),
        1,
        "a real, running ambient heartbeat must carry a node added after it started, with no \
         manual sync call"
    );

    heartbeat.stop();
    server.stop();
}
