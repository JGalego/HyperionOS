//! Mirrors every other crate in this workspace: every call is capability-
//! gated, re-checked live against the monitor.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_plugin_framework::{
    signature, CapabilityManifest, Contribution, ImplementationKind, PluginError, PluginManifest,
    PluginRegistry, SemanticContract, SideEffect, TrustDepth,
};

fn manifest() -> PluginManifest {
    let mut m = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: 0,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "document.summarize".to_string(),
            contract: SemanticContract {
                inputs: vec!["text".to_string()],
                outputs: vec!["summary".to_string()],
                side_effects: vec![SideEffect::None],
            },
            implementation_kind: ImplementationKind::LocalSmallModel,
            version: 1,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    m.signature = signature(&m);
    m
}

#[test]
fn install_requires_grant_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let no_grant = monitor
        .cap_derive(
            &root,
            RightsMask::READ | RightsMask::WRITE,
            None,
            TrustBoundaryId(2),
        )
        .unwrap();
    let registry = PluginRegistry::new();

    let result = registry.install(
        &mut monitor,
        &no_grant,
        manifest(),
        TrustDepth::D0,
        true,
        1_000,
    );
    assert!(matches!(result, Err(PluginError::Unauthorized)));
}

#[test]
fn uninstall_requires_revoke_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let handle = registry
        .install(&mut monitor, &root, manifest(), TrustDepth::D0, true, 1_000)
        .unwrap();

    let no_revoke = monitor
        .cap_derive(
            &root,
            RightsMask::READ | RightsMask::WRITE | RightsMask::GRANT,
            None,
            TrustBoundaryId(2),
        )
        .unwrap();
    let result = registry.uninstall(&mut monitor, &no_revoke, handle.plugin_id);
    assert!(matches!(result, Err(PluginError::Unauthorized)));
}

#[test]
fn revoking_the_admin_token_blocks_further_installs_re_checked_live() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let delegate = monitor
        .cap_derive(&root, RightsMask::all(), None, TrustBoundaryId(2))
        .unwrap();
    let registry = PluginRegistry::new();

    assert!(registry
        .install(
            &mut monitor,
            &delegate,
            manifest(),
            TrustDepth::D0,
            true,
            1_000
        )
        .is_ok());

    monitor.cap_revoke(&delegate);

    let mut second = manifest();
    second.plugin_id = 2;
    second.signature = signature(&second);
    assert!(matches!(
        registry.install(&mut monitor, &delegate, second, TrustDepth::D0, true, 1_001),
        Err(PluginError::Unauthorized)
    ));
}
