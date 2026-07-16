//! Mirrors every other crate in this workspace: every call is capability-
//! gated, re-checked live against the monitor.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{
    sign, CapabilityManifest, Contribution, ImplementationKind, PluginError, PluginManifest,
    PluginRegistry, PrivacyTier, SemanticContract, SideEffect, TrustDepth,
};

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

fn manifest(keystore: &Keystore) -> PluginManifest {
    let mut m = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "document.summarize".to_string(),
            contract: SemanticContract {
                inputs: vec!["text".to_string()],
                outputs: vec!["summary".to_string()],
                side_effects: vec![SideEffect::None],
            },
            implementation_kind: ImplementationKind::LocalSmallModel,
            quality_score: 0.5,
            version: 1,
            native_binary: None,
            privacy_tier: PrivacyTier::Local,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    m.signature = Some(sign(&m, keystore));
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
    let (_dir, keystore) = keystore();

    let result = registry.install(
        &mut monitor,
        &no_grant,
        manifest(&keystore),
        TrustDepth::D0,
        true,
        1_000,
        &keystore.verifying_key(),
    );
    assert!(matches!(result, Err(PluginError::Unauthorized)));
}

#[test]
fn uninstall_requires_revoke_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();
    let handle = registry
        .install(
            &mut monitor,
            &root,
            manifest(&keystore),
            TrustDepth::D0,
            true,
            1_000,
            &keystore.verifying_key(),
        )
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
    let (_dir, keystore) = keystore();

    assert!(registry
        .install(
            &mut monitor,
            &delegate,
            manifest(&keystore),
            TrustDepth::D0,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .is_ok());

    monitor.cap_revoke(&delegate);

    let mut second = manifest(&keystore);
    second.plugin_id = 2;
    second.signature = Some(sign(&second, &keystore));
    assert!(matches!(
        registry.install(
            &mut monitor,
            &delegate,
            second,
            TrustDepth::D0,
            true,
            1_001,
            &keystore.verifying_key(),
        ),
        Err(PluginError::Unauthorized)
    ));
}
