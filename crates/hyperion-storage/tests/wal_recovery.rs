//! docs/28-storage-engine.md §Testing Strategy: "Crash-consistency is
//! tested by fault injection at each of the four write-path phases...
//! asserting that replay always converges to a state consistent with the
//! last durably committed WAL record — never partial."

use std::fs::OpenOptions;
use std::io::Write;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_storage::StorageEngine;
use serde_json::json;

#[test]
fn all_committed_writes_survive_a_clean_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let wal_path = dir.path().join("wal.jsonl");

    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);

    let (obj_a, obj_b) = {
        let engine = StorageEngine::open(&wal_path).unwrap();
        let (obj_a, _) = engine
            .put_object(
                &monitor,
                &token,
                None,
                None,
                json!({"name": "vacation photo"}),
            )
            .unwrap();
        let (obj_b, _) = engine
            .put_object(
                &monitor,
                &token,
                None,
                None,
                json!({"name": "trip itinerary"}),
            )
            .unwrap();
        // A second write to obj_a, so recovery must reconstruct the *latest*
        // version, not just the first one ever written for that object.
        let v1 = engine.current_version(obj_a).unwrap();
        engine
            .put_object(
                &monitor,
                &token,
                Some(obj_a),
                Some(v1),
                json!({"name": "vacation photo (edited)"}),
            )
            .unwrap();
        (obj_a, obj_b)
    }; // engine dropped here — nothing special about drop, WAL is already durable per-write

    let recovered = StorageEngine::open(&wal_path).unwrap();
    assert_eq!(
        recovered.get_object(&monitor, &token, obj_a, None).unwrap(),
        json!({"name": "vacation photo (edited)"})
    );
    assert_eq!(
        recovered.get_object(&monitor, &token, obj_b, None).unwrap(),
        json!({"name": "trip itinerary"})
    );
}

#[test]
fn recovery_tolerates_a_torn_trailing_record_without_losing_prior_writes() {
    let dir = tempfile::tempdir().unwrap();
    let wal_path = dir.path().join("wal.jsonl");

    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);

    let obj_a = {
        let engine = StorageEngine::open(&wal_path).unwrap();
        let (obj_a, _) = engine
            .put_object(
                &monitor,
                &token,
                None,
                None,
                json!({"name": "committed before the crash"}),
            )
            .unwrap();
        obj_a
    };

    // Simulate a crash mid-append: a real WAL append that never completed
    // its fsync would leave exactly this shape on disk — a torn, incomplete
    // trailing line with no newline terminator. The exact bytes don't need
    // to resemble a real (truncated) WalRecord encoding, only to be
    // unparseable as one, which any partial write necessarily is.
    {
        let mut file = OpenOptions::new().append(true).open(&wal_path).unwrap();
        write!(file, "{{\"object_id\":{{\"0\":99}},\"prev_ver").unwrap();
    }

    let recovered = StorageEngine::open(&wal_path)
        .expect("replay must tolerate a torn trailing record, not fail to open");
    assert_eq!(
        recovered.get_object(&monitor, &token, obj_a, None).unwrap(),
        json!({"name": "committed before the crash"}),
        "the last durably committed record must still be recovered"
    );
    assert!(
        recovered
            .current_version(hyperion_storage::ObjectId(99))
            .is_none(),
        "the torn record must be treated as if it never happened, not partially applied"
    );
}
