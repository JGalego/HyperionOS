//! docs/17 T2: a compromised Plugin/Capability's authority must fence off
//! instantly, and completely, down its entire delegation subtree.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};

#[test]
fn t2_revoking_a_compromised_plugins_token_cascades_to_every_capability_it_derived() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let plugin = monitor
        .cap_derive(&root, RightsMask::all(), None, TrustBoundaryId(2))
        .unwrap();
    let sub_capability = monitor
        .cap_derive(&plugin, RightsMask::EXEC, None, TrustBoundaryId(3))
        .unwrap();

    assert!(monitor.check_rights_ok(&sub_capability, RightsMask::EXEC));

    monitor.cap_revoke(&plugin);

    assert!(!monitor.check_rights_ok(&sub_capability, RightsMask::EXEC), "a plugin's revocation must cascade to every capability it derived, not just the plugin's own token");
    assert!(!monitor.check_rights_ok(&plugin, RightsMask::EXEC));
}

#[test]
fn t2_a_sibling_delegation_from_the_same_root_is_unaffected() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let compromised_plugin = monitor
        .cap_derive(&root, RightsMask::all(), None, TrustBoundaryId(2))
        .unwrap();
    let unrelated_plugin = monitor
        .cap_derive(&root, RightsMask::all(), None, TrustBoundaryId(3))
        .unwrap();

    monitor.cap_revoke(&compromised_plugin);

    assert!(
        monitor.check_rights_ok(&unrelated_plugin, RightsMask::EXEC),
        "revoking one compromised plugin must not collaterally revoke an unrelated sibling"
    );
}
