//! Mirrors every other crate in this workspace: every call is capability-
//! gated, re-checked live against the monitor.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_device::{DeviceError, DeviceRegistry, DeviceType, TrustTier};

#[test]
fn register_requires_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();

    let registry = DeviceRegistry::new();
    let result = registry.register(
        &monitor,
        &read_only,
        DeviceType::Display,
        "Acme",
        "D1",
        vec![],
        1,
        0,
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

    let registry = DeviceRegistry::new();
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
