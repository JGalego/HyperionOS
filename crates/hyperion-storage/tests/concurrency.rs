//! docs/28-storage-engine.md §Testing Strategy: "Concurrency stress tests
//! run many simulated writers against the same object to validate the
//! compare-and-swap version-pointer path never lets two writers both
//! believe they won."

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_storage::{StorageEngine, StorageError};
use serde_json::json;

#[test]
fn stale_expected_version_is_rejected_not_silently_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let engine = StorageEngine::open(dir.path().join("wal.jsonl")).unwrap();

    let (obj, v1) = engine
        .put_object(&monitor, &token, None, None, json!({"draft": 1}))
        .unwrap();

    // Writer A reads v1 and prepares an update against it...
    // ...but Writer B commits first, advancing the object past v1.
    let (_, v2) = engine
        .put_object(&monitor, &token, Some(obj), Some(v1), json!({"draft": 2}))
        .unwrap();
    assert_ne!(v1, v2);

    // Writer A's write, still believing v1 is current, must be rejected —
    // never silently applied on top of Writer B's already-committed change.
    let result = engine.put_object(
        &monitor,
        &token,
        Some(obj),
        Some(v1),
        json!({"draft": "A's stale write"}),
    );
    match result {
        Err(StorageError::ConcurrentWriteConflict {
            object_id,
            expected,
            found,
        }) => {
            assert_eq!(object_id, obj);
            assert_eq!(expected, Some(v1));
            assert_eq!(found, Some(v2));
        }
        other => panic!("expected ConcurrentWriteConflict, got {other:?}"),
    }

    // The object must still reflect Writer B's committed value, untouched
    // by Writer A's rejected attempt.
    assert_eq!(
        engine.get_object(&monitor, &token, obj, None).unwrap(),
        json!({"draft": 2})
    );
}

#[test]
fn many_concurrent_writers_racing_the_same_object_never_lose_or_double_count_a_write() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let engine = Arc::new(StorageEngine::open(dir.path().join("wal.jsonl")).unwrap());
    let monitor = Arc::new(monitor);
    let token = Arc::new(token);

    let (obj, _) = engine
        .put_object(&monitor, &token, None, None, json!({"counter": 0}))
        .unwrap();

    const WRITERS: usize = 8;
    const ATTEMPTS_PER_WRITER: usize = 50;
    let successes = Arc::new(AtomicU32::new(0));

    let handles: Vec<_> = (0..WRITERS)
        .map(|w| {
            let engine = Arc::clone(&engine);
            let monitor = Arc::clone(&monitor);
            let token = Arc::clone(&token);
            let successes = Arc::clone(&successes);
            thread::spawn(move || {
                for _ in 0..ATTEMPTS_PER_WRITER {
                    // Read-modify-write with retry on conflict — the caller's
                    // responsibility per docs/28 §Algorithms ("the caller
                    // retries against the new head"), not the engine's.
                    loop {
                        let current = engine.current_version(obj);
                        match engine.put_object(
                            &monitor,
                            &token,
                            Some(obj),
                            current,
                            json!({"writer": w}),
                        ) {
                            Ok(_) => {
                                successes.fetch_add(1, Ordering::SeqCst);
                                break;
                            }
                            Err(StorageError::ConcurrentWriteConflict { .. }) => continue,
                            Err(e) => panic!("unexpected error: {e}"),
                        }
                    }
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    let total_successes = successes.load(Ordering::SeqCst);
    assert_eq!(total_successes as usize, WRITERS * ATTEMPTS_PER_WRITER);

    // The version pointer only ever advances by exactly one per successful
    // write (the initial create plus every retry-until-success above) — if
    // two writers had ever both believed a stale version was current, the
    // final version number would fall short of this count.
    let final_version = engine.current_version(obj).unwrap();
    assert_eq!(final_version.0 as usize, WRITERS * ATTEMPTS_PER_WRITER);
}
