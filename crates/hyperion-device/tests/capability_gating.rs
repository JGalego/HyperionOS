//! Mirrors every other crate in this workspace: every call is capability-
//! gated, re-checked live against the monitor.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_device::{DeviceError, DeviceRegistry, DeviceType, TrustTier};
use hyperion_knowledge_graph::KnowledgeGraph;

fn registry() -> (tempfile::TempDir, DeviceRegistry, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, DeviceRegistry::new(graph), keystore)
}

#[test]
fn register_requires_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();

    let (_dir, registry, keystore) = registry();
    let signature = hyperion_device::sign(DeviceType::Display, "Acme", "D1", &[], 1, &keystore);
    let result = registry.register(
        &monitor,
        &read_only,
        DeviceType::Display,
        "Acme",
        "D1",
        vec![],
        1,
        0,
        &signature,
        &keystore.verifying_key(),
    );
    assert!(matches!(result, Err(DeviceError::Unauthorized)));
}

#[test]
fn revoking_a_token_blocks_further_access_re_checked_live() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let delegate = monitor
        .cap_derive(&root, RightsMask::all(), None, TrustBoundaryId(2))
        .unwrap();

    let (_dir, registry, keystore) = registry();
    let signature = hyperion_device::sign(DeviceType::Display, "Acme", "D1", &[], 1, &keystore);
    let device = registry
        .register(
            &monitor,
            &delegate,
            DeviceType::Display,
            "Acme",
            "D1",
            vec![],
            1,
            0,
            &signature,
            &keystore.verifying_key(),
        )
        .unwrap();
    assert!(registry
        .pair(&monitor, &delegate, device, TrustTier::View, vec![], false)
        .is_ok());

    monitor.cap_revoke(&delegate);

    assert!(matches!(
        registry.pair(&monitor, &delegate, device, TrustTier::View, vec![], false),
        Err(DeviceError::Unauthorized)
    ));
}
