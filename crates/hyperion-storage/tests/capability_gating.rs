//! docs/28-storage-engine.md §Security Considerations: "Every call...
//! carries a capability token scoped to a specific object, object type, or
//! query shape; the ACL... is re-evaluated on every get_object... never
//! cached across a Trust Boundary."

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_storage::{StorageEngine, StorageError};
use serde_json::json;

#[test]
fn put_object_requires_write_rights() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();

    let engine = StorageEngine::open(dir.path().join("wal.jsonl")).unwrap();
    let result = engine.put_object(&monitor, &read_only, None, None, json!({"x": 1}));
    assert!(matches!(result, Err(StorageError::Unauthorized)));
}

#[test]
fn get_object_requires_read_rights() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let engine = StorageEngine::open(dir.path().join("wal.jsonl")).unwrap();
    let (obj, _) = engine
        .put_object(&monitor, &root, None, None, json!({"secret": true}))
        .unwrap();

    let write_only = monitor
        .cap_derive(&root, RightsMask::WRITE, None, TrustBoundaryId(2))
        .unwrap();
    let result = engine.get_object(&monitor, &write_only, obj, None);
    assert!(matches!(result, Err(StorageError::Unauthorized)));
}

#[test]
fn revoking_a_token_blocks_further_access_re_checked_live() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let delegate = monitor
        .cap_derive(
            &root,
            RightsMask::READ | RightsMask::WRITE,
            None,
            TrustBoundaryId(2),
        )
        .unwrap();

    let engine = StorageEngine::open(dir.path().join("wal.jsonl")).unwrap();
    let (obj, _) = engine
        .put_object(&monitor, &delegate, None, None, json!({"n": 1}))
        .unwrap();
    assert!(engine.get_object(&monitor, &delegate, obj, None).is_ok());

    monitor.cap_revoke(&delegate);

    // The same token value, re-checked against the *live* monitor state —
    // not a cached liveness result from when it was minted.
    assert!(matches!(
        engine.get_object(&monitor, &delegate, obj, None),
        Err(StorageError::Unauthorized)
    ));
    assert!(matches!(
        engine.put_object(
            &monitor,
            &delegate,
            Some(obj),
            engine.current_version(obj),
            json!({"n": 2})
        ),
        Err(StorageError::Unauthorized)
    ));
}
