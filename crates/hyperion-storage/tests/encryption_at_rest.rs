//! docs/28-storage-engine.md's own named "no encryption at rest" gap, closed for a caller that
//! opts in via `StorageEngine::open_encrypted`: every WAL record on disk is a real, individually
//! AEAD-sealed, hex-encoded line -- never plaintext JSON -- and a wrong key fails closed rather
//! than silently returning wrong or garbage data.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_storage::StorageEngine;
use serde_json::json;

const KEY_A: [u8; 32] = [7u8; 32];
const KEY_B: [u8; 32] = [9u8; 32];

#[test]
fn an_encrypted_wal_never_contains_the_plaintext_metadata_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let wal_path = dir.path().join("wal.jsonl");

    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);

    let engine = StorageEngine::open_encrypted(&wal_path, KEY_A).unwrap();
    engine
        .put_object(
            &monitor,
            &token,
            None,
            None,
            json!({"name": "a very secret vacation photo"}),
        )
        .unwrap();
    drop(engine);

    let raw = std::fs::read_to_string(&wal_path).unwrap();
    assert!(
        !raw.contains("very secret vacation photo"),
        "the real plaintext must never appear on disk once encryption at rest is enabled: {raw:?}"
    );
    assert!(
        !raw.contains('{'),
        "an encrypted WAL line must be hex, not plaintext JSON: {raw:?}"
    );
}

#[test]
fn an_encrypted_wal_round_trips_through_a_real_reopen_with_the_same_key() {
    let dir = tempfile::tempdir().unwrap();
    let wal_path = dir.path().join("wal.jsonl");

    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);

    let obj = {
        let engine = StorageEngine::open_encrypted(&wal_path, KEY_A).unwrap();
        let (obj, _) = engine
            .put_object(
                &monitor,
                &token,
                None,
                None,
                json!({"name": "trip itinerary"}),
            )
            .unwrap();
        obj
    };

    let recovered = StorageEngine::open_encrypted(&wal_path, KEY_A)
        .expect("reopening with the same real key must succeed");
    assert_eq!(
        recovered.get_object(&monitor, &token, obj, None).unwrap(),
        json!({"name": "trip itinerary"})
    );
}

#[test]
fn opening_an_encrypted_wal_with_the_wrong_key_recovers_nothing_not_garbage() {
    let dir = tempfile::tempdir().unwrap();
    let wal_path = dir.path().join("wal.jsonl");

    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);

    let obj = {
        let engine = StorageEngine::open_encrypted(&wal_path, KEY_A).unwrap();
        let (obj, _) = engine
            .put_object(
                &monitor,
                &token,
                None,
                None,
                json!({"name": "trip itinerary"}),
            )
            .unwrap();
        obj
    };

    // A real AEAD authentication failure on every line looks exactly like an empty/torn WAL to
    // replay -- fails closed (nothing recovered), never decrypts to wrong or garbage content.
    let wrong_key = StorageEngine::open_encrypted(&wal_path, KEY_B)
        .expect("opening must still succeed -- the WAL is just treated as empty");
    assert!(
        wrong_key.current_version(obj).is_none(),
        "the wrong key must recover none of the real data, not garbage"
    );
}

#[test]
fn compaction_stays_encrypted_and_survives_a_real_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let wal_path = dir.path().join("wal.jsonl");

    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);

    let obj = {
        let engine = StorageEngine::open_encrypted(&wal_path, KEY_A).unwrap();
        let (obj, _) = engine
            .put_object(&monitor, &token, None, None, json!({"name": "v1"}))
            .unwrap();
        let v1 = engine.current_version(obj).unwrap();
        engine
            .put_object(&monitor, &token, Some(obj), Some(v1), json!({"name": "v2"}))
            .unwrap();
        engine.compact(&monitor, &token).unwrap();
        obj
    };

    let raw = std::fs::read_to_string(&wal_path).unwrap();
    assert!(
        !raw.contains("v1") && !raw.contains("v2"),
        "a compacted, encrypted WAL must still never contain plaintext on disk: {raw:?}"
    );

    let recovered = StorageEngine::open_encrypted(&wal_path, KEY_A)
        .expect("reopening a compacted, encrypted WAL with the same key must succeed");
    assert_eq!(
        recovered.get_object(&monitor, &token, obj, None).unwrap(),
        json!({"name": "v2"}),
        "compaction must keep the current head readable after a real reopen"
    );
}
