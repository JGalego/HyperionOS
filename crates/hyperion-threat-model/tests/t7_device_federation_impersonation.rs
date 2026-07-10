//! docs/17 T7: device-federation impersonation / split-brain — a
//! compromised or stale device must lose a conflicting anchor claim to a
//! more-trusted device, and revoking a paired device must block
//! invocation immediately.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_device::{
    CapabilityManifestEntry, DeviceRegistry, DeviceType, Direction, SafetyClass,
    TrustTier as DeviceTrustTier,
};
use hyperion_federation::{FederationHub, FederationTrustTier};
use hyperion_knowledge_graph::KnowledgeGraph;

#[test]
fn t7_a_more_trusted_device_wins_a_conflicting_anchor_claim_over_a_stale_holder() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let hub = FederationHub::new();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::SharedHousehold)
        .unwrap();
    hub.join_device(&monitor, &root, 2, FederationTrustTier::OwnedPrimary)
        .unwrap();

    hub.acquire_lease(&monitor, &root, 42, 1, 1_000, 60)
        .unwrap();
    let lease = hub
        .acquire_lease(&monitor, &root, 42, 2, 1_005, 60)
        .unwrap();

    assert_eq!(lease.holder_device, 2, "device 1 (compromised or merely stale) must lose the anchor claim to the more-trusted device 2");
}

#[test]
fn t7_revoking_a_paired_device_immediately_blocks_further_invocation() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let registry = DeviceRegistry::new(graph);
    let device = registry
        .register(
            &monitor,
            &root,
            DeviceType::Display,
            "Acme",
            "D1",
            vec![CapabilityManifestEntry {
                capability_name: "render".to_string(),
                direction: Direction::Render,
                safety_class: SafetyClass::Cosmetic,
            }],
            1,
            0,
        )
        .unwrap();
    registry
        .pair(
            &monitor,
            &root,
            device,
            DeviceTrustTier::View,
            vec!["render".to_string()],
            false,
        )
        .unwrap();
    assert!(registry
        .invoke(&monitor, &root, device, "render", serde_json::json!({}))
        .is_ok());

    registry.revoke(&monitor, &root, device).unwrap();

    let result = registry.invoke(&monitor, &root, device, "render", serde_json::json!({}));
    assert!(
        result.is_err(),
        "a revoked device must not be invocable, closing the impersonation window instantly"
    );
}
