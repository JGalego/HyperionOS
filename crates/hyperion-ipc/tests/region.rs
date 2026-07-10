//! docs/30-ipc-framework.md §Testing Strategy: "property tests asserting no
//! receiver can observe a region after its capability's generation has been
//! bumped (use-after-revoke)."

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_ipc::{region_map, region_share, IpcFault};

#[test]
fn region_map_succeeds_while_live_and_fails_after_revoke() {
    let mut monitor = CapabilityMonitor::new();
    let cap = monitor.mint_root(RightsMask::READ | RightsMask::MAP, TrustBoundaryId(1), None);
    let desc = region_share(cap.clone(), b"large semantic object blob".to_vec());
    assert_eq!(desc.len(), b"large semantic object blob".len());

    let mapped = region_map(&monitor, &desc, RightsMask::READ).expect("live region must map");
    assert_eq!(&*mapped, b"large semantic object blob");

    monitor.cap_revoke(&cap);
    let result = region_map(&monitor, &desc, RightsMask::READ);
    assert_eq!(
        result.unwrap_err(),
        IpcFault::Kernel(hyperion_capability::Fault::Revoked)
    );
}

#[test]
fn region_map_rejects_insufficient_rights() {
    let mut monitor = CapabilityMonitor::new();
    let cap = monitor.mint_root(RightsMask::READ, TrustBoundaryId(1), None);
    let desc = region_share(cap, b"read-only blob".to_vec());
    let result = region_map(&monitor, &desc, RightsMask::WRITE);
    assert!(matches!(result, Err(IpcFault::Kernel(_))));
}
