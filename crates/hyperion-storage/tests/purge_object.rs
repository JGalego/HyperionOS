//! `hyperion-privacy`'s own named "still not a byte-level deletion from the WAL's history" gap,
//! closed here: `StorageEngine::purge_object` removes every WAL record one specific object ever
//! had -- including its current head -- while every other object's own full history survives
//! untouched, unlike `StorageEngine::compact`'s own blanket collapse-everything-to-head sweep.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_storage::{StorageEngine, StorageError};
use serde_json::json;

#[test]
fn purge_object_requires_write_rights() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();
    let engine = StorageEngine::open(dir.path().join("wal.jsonl")).unwrap();

    let (obj, _) = engine
        .put_object(&monitor, &root, None, None, json!({"name": "v1"}))
        .unwrap();

    let result = engine.purge_object(&monitor, &read_only, obj);
    assert!(matches!(result, Err(StorageError::Unauthorized)));
}

#[test]
fn purging_an_unknown_object_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let engine = StorageEngine::open(dir.path().join("wal.jsonl")).unwrap();

    let result = engine.purge_object(&monitor, &root, hyperion_storage::ObjectId(9999));
    assert!(matches!(result, Err(StorageError::NotFound)));
}

#[test]
fn purging_removes_every_historical_version_including_the_current_head() {
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

    let purged = engine.purge_object(&monitor, &root, obj).unwrap();
    assert_eq!(
        purged, 3,
        "all three real historical versions must be counted"
    );

    assert!(matches!(
        engine.get_object(&monitor, &root, obj, None),
        Err(StorageError::NotFound)
    ));
    assert!(matches!(
        engine.get_object(&monitor, &root, obj, Some(v1)),
        Err(StorageError::NotFound)
    ));
    assert!(matches!(
        engine.get_object(&monitor, &root, obj, Some(v2)),
        Err(StorageError::NotFound)
    ));
    assert_eq!(engine.current_version(obj), None);
}

#[test]
fn purging_one_object_never_touches_another_objects_full_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let engine = StorageEngine::open(dir.path().join("wal.jsonl")).unwrap();

    let (target, t1) = engine
        .put_object(&monitor, &root, None, None, json!({"name": "target-v1"}))
        .unwrap();
    engine
        .put_object(
            &monitor,
            &root,
            Some(target),
            Some(t1),
            json!({"name": "target-v2"}),
        )
        .unwrap();

    let (survivor, s1) = engine
        .put_object(&monitor, &root, None, None, json!({"name": "survivor-v1"}))
        .unwrap();
    let (_, s2) = engine
        .put_object(
            &monitor,
            &root,
            Some(survivor),
            Some(s1),
            json!({"name": "survivor-v2"}),
        )
        .unwrap();

    engine.purge_object(&monitor, &root, target).unwrap();

    // The survivor's own full history -- both versions -- must remain completely intact.
    assert_eq!(
        engine.get_object(&monitor, &root, survivor, None).unwrap(),
        json!({"name": "survivor-v2"})
    );
    assert_eq!(
        engine
            .get_object(&monitor, &root, survivor, Some(s1))
            .unwrap(),
        json!({"name": "survivor-v1"}),
        "purging a different object must never collapse the survivor's own history"
    );
    assert_eq!(engine.current_version(survivor), Some(s2));
}

#[test]
fn purging_is_durable_across_a_real_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let wal_path = dir.path().join("wal.jsonl");
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);

    let (target, survivor, s1) = {
        let engine = StorageEngine::open(&wal_path).unwrap();
        let (target, t1) = engine
            .put_object(&monitor, &root, None, None, json!({"name": "target"}))
            .unwrap();
        engine
            .put_object(
                &monitor,
                &root,
                Some(target),
                Some(t1),
                json!({"name": "target-v2"}),
            )
            .unwrap();
        let (survivor, s1) = engine
            .put_object(&monitor, &root, None, None, json!({"name": "survivor"}))
            .unwrap();
        engine.purge_object(&monitor, &root, target).unwrap();
        (target, survivor, s1)
    };

    let recovered = StorageEngine::open(&wal_path).unwrap();
    assert!(
        matches!(
            recovered.get_object(&monitor, &root, target, None),
            Err(StorageError::NotFound)
        ),
        "a real restart must never resurrect a purged object's history from the rewritten WAL"
    );
    assert_eq!(
        recovered
            .get_object(&monitor, &root, survivor, None)
            .unwrap(),
        json!({"name": "survivor"})
    );
    assert_eq!(recovered.current_version(survivor), Some(s1));
}

#[test]
fn purging_an_already_purged_object_is_not_found_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let engine = StorageEngine::open(dir.path().join("wal.jsonl")).unwrap();

    let (obj, _) = engine
        .put_object(&monitor, &root, None, None, json!({"name": "v1"}))
        .unwrap();
    engine.purge_object(&monitor, &root, obj).unwrap();

    let result = engine.purge_object(&monitor, &root, obj);
    assert!(matches!(result, Err(StorageError::NotFound)));
}

#[test]
fn purging_works_under_real_encryption_at_rest_too() {
    let dir = tempfile::tempdir().unwrap();
    let wal_path = dir.path().join("wal.jsonl");
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let key = [7u8; 32];
    let engine = StorageEngine::open_encrypted(&wal_path, key).unwrap();

    let (target, _) = engine
        .put_object(&monitor, &root, None, None, json!({"name": "secret"}))
        .unwrap();
    let (survivor, _) = engine
        .put_object(&monitor, &root, None, None, json!({"name": "keep"}))
        .unwrap();

    engine.purge_object(&monitor, &root, target).unwrap();

    let recovered = StorageEngine::open_encrypted(&wal_path, key).unwrap();
    assert!(matches!(
        recovered.get_object(&monitor, &root, target, None),
        Err(StorageError::NotFound)
    ));
    assert_eq!(
        recovered
            .get_object(&monitor, &root, survivor, None)
            .unwrap(),
        json!({"name": "keep"})
    );
}
