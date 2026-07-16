//! This crate's own named "garbage collection / compaction" gap, real for the first time (see
//! the crate doc comment): `StorageEngine::compact` collapses every object's version chain
//! unconditionally to its current head, rewriting the on-disk WAL itself, not just the
//! in-memory view — a real restart must not resurrect history it already dropped.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_storage::{StorageEngine, StorageError};
use serde_json::json;

#[test]
fn compact_requires_write_rights() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();

    let engine = StorageEngine::open(dir.path().join("wal.jsonl")).unwrap();
    let result = engine.compact(&monitor, &read_only);
    assert!(matches!(result, Err(StorageError::Unauthorized)));
}

#[test]
fn compacting_collapses_history_but_keeps_the_current_head_readable() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let engine = StorageEngine::open(dir.path().join("wal.jsonl")).unwrap();

    let (obj, v1) = engine
        .put_object(&monitor, &root, None, None, json!({"name": "v1"}))
        .unwrap();
    let (_, v2) = engine
        .put_object(&monitor, &root, Some(obj), Some(v1), json!({"name": "v2"}))
        .unwrap();
    engine
        .put_object(&monitor, &root, Some(obj), Some(v2), json!({"name": "v3"}))
        .unwrap();

    let evicted = engine.compact(&monitor, &root).unwrap();
    assert_eq!(
        evicted, 2,
        "v1 and v2 must be pruned, only the head (v3) survives"
    );

    assert_eq!(
        engine.get_object(&monitor, &root, obj, None).unwrap(),
        json!({"name": "v3"}),
        "the current head must still be readable after compaction"
    );
    assert!(
        matches!(
            engine.get_object(&monitor, &root, obj, Some(v1)),
            Err(StorageError::NotFound)
        ),
        "a pruned historical version must no longer be reachable"
    );
    assert!(matches!(
        engine.get_object(&monitor, &root, obj, Some(v2)),
        Err(StorageError::NotFound)
    ));
}

#[test]
fn compaction_is_durable_across_a_real_reopen_not_just_an_in_memory_prune() {
    let dir = tempfile::tempdir().unwrap();
    let wal_path = dir.path().join("wal.jsonl");
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);

    let (obj, head) = {
        let engine = StorageEngine::open(&wal_path).unwrap();
        let (obj, v1) = engine
            .put_object(&monitor, &root, None, None, json!({"name": "v1"}))
            .unwrap();
        let (_, v2) = engine
            .put_object(&monitor, &root, Some(obj), Some(v1), json!({"name": "v2"}))
            .unwrap();
        engine.compact(&monitor, &root).unwrap();
        (obj, v2)
    };

    let recovered = StorageEngine::open(&wal_path).unwrap();
    assert_eq!(
        recovered.get_object(&monitor, &root, obj, None).unwrap(),
        json!({"name": "v2"}),
        "the head must survive a real reopen"
    );
    assert_eq!(recovered.current_version(obj), Some(head));
    assert!(
        recovered
            .get_object(&monitor, &root, obj, Some(head))
            .is_ok(),
        "the head version must remain addressable by its own id after replay"
    );
}

#[test]
fn compacting_twice_in_a_row_evicts_nothing_the_second_time() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let engine = StorageEngine::open(dir.path().join("wal.jsonl")).unwrap();

    let (obj, v1) = engine
        .put_object(&monitor, &root, None, None, json!({"name": "v1"}))
        .unwrap();
    engine
        .put_object(&monitor, &root, Some(obj), Some(v1), json!({"name": "v2"}))
        .unwrap();

    let first = engine.compact(&monitor, &root).unwrap();
    assert_eq!(first, 1);
    let second = engine.compact(&monitor, &root).unwrap();
    assert_eq!(
        second, 0,
        "an already-collapsed chain has nothing left to prune"
    );
}

#[test]
fn a_write_after_compaction_still_honors_the_compare_and_swap_invariant() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let engine = StorageEngine::open(dir.path().join("wal.jsonl")).unwrap();

    let (obj, v1) = engine
        .put_object(&monitor, &root, None, None, json!({"name": "v1"}))
        .unwrap();
    engine
        .put_object(&monitor, &root, Some(obj), Some(v1), json!({"name": "v2"}))
        .unwrap();
    engine.compact(&monitor, &root).unwrap();

    let head = engine.current_version(obj).unwrap();

    // A stale expected_version (the now-pruned v1) must still be rejected as a conflict.
    let stale = engine.put_object(
        &monitor,
        &root,
        Some(obj),
        Some(v1),
        json!({"name": "stale"}),
    );
    assert!(matches!(
        stale,
        Err(StorageError::ConcurrentWriteConflict { .. })
    ));

    // The real current head must still work as expected_version for a fresh write.
    let (_, v3) = engine
        .put_object(
            &monitor,
            &root,
            Some(obj),
            Some(head),
            json!({"name": "v3"}),
        )
        .unwrap();
    assert_eq!(
        engine.get_object(&monitor, &root, obj, None).unwrap(),
        json!({"name": "v3"})
    );
    assert_eq!(engine.current_version(obj), Some(v3));
}
