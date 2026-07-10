//! Mirrors every other crate in this workspace: every call is capability-
//! gated, re-checked live against the monitor.

use hyperion_agent_runtime::{AgentManifest, TrustTier};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_federation::{FederationError, FederationHub, FederationTrustTier};

#[test]
fn join_device_requires_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();

    let hub = FederationHub::new();
    let result = hub.join_device(&monitor, &read_only, 1, FederationTrustTier::OwnedPrimary);
    assert!(matches!(result, Err(FederationError::Unauthorized)));
}

#[test]
fn dispatch_offload_requires_exec_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let write_only = monitor
        .cap_derive(&root, RightsMask::WRITE, None, TrustBoundaryId(2))
        .unwrap();

    let hub = FederationHub::new();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    hub.publish_ledger(
        1,
        hyperion_scheduler::ResourceVector::default(),
        5,
        1_000,
        60,
    )
    .unwrap();

    let descriptor = hyperion_federation::OffloadDescriptor {
        request: hyperion_scheduler::ResourceVector::default(),
        deadline_ms: None,
        privacy_tier: hyperion_federation::PrivacyTier::Local,
    };
    let result = hub.dispatch_offload(
        &monitor,
        &write_only,
        &descriptor,
        "web.search",
        serde_json::json!({}),
        1_000,
    );
    assert!(matches!(result, Err(FederationError::Unauthorized)));
}

#[test]
fn revoking_a_token_blocks_further_access_re_checked_live() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let delegate = monitor
        .cap_derive(&root, RightsMask::all(), None, TrustBoundaryId(2))
        .unwrap();

    let hub = FederationHub::new();
    assert!(hub
        .join_device(&monitor, &delegate, 1, FederationTrustTier::OwnedPrimary)
        .is_ok());

    monitor.cap_revoke(&delegate);

    let manifest = AgentManifest {
        specialization: "test".to_string(),
        baseline_capabilities: vec![],
        requestable_capabilities: vec![],
        trust_tier: TrustTier::System,
    };
    assert!(matches!(
        hub.spawn_agent(&monitor, &delegate, 1, manifest, None, 1_000, 60),
        Err(FederationError::Unauthorized)
    ));
}
