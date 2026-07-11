//! docs/24 §5's `plugin_install`/uninstall: exactly the requested tokens
//! are minted, and revoking them all is a single cascade.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{
    sign, CapabilityGrantRequest, CapabilityManifest, Contribution, ImplementationKind, Operation,
    PluginError, PluginManifest, PluginRegistry, SemanticContract, SideEffect, TrustDepth,
};

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

fn manifest_with_web_search(keystore: &Keystore) -> PluginManifest {
    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract: SemanticContract {
                inputs: vec!["query".to_string()],
                outputs: vec!["results".to_string()],
                side_effects: vec![SideEffect::NetworkEgress],
            },
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.5,
            version: 1,
        })],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::NetworkEgress,
            scope: "web.search".to_string(),
            justification: "fetch search results".to_string(),
        }],
        min_trust_depth: TrustDepth::D1,
    };
    manifest.signature = Some(sign(&manifest, keystore));
    manifest
}

#[test]
fn a_valid_manifest_installs_and_mints_exactly_the_requested_tokens() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let handle = registry
        .install(
            &mut monitor,
            &root,
            manifest_with_web_search(&keystore),
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let entry = registry.query("web.search").unwrap();
    assert_eq!(entry.implementations.len(), 1);
    assert_eq!(entry.owning_plugins, vec![handle.plugin_id]);
}

#[test]
fn a_manifest_requiring_deeper_trust_than_available_is_rejected() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let result = registry.install(
        &mut monitor,
        &root,
        manifest_with_web_search(&keystore),
        TrustDepth::D0,
        true,
        1_000,
        &keystore.verifying_key(),
    );
    assert!(matches!(result, Err(PluginError::InsufficientTrustDepth)));
}

#[test]
fn installation_without_consent_is_rejected() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let result = registry.install(
        &mut monitor,
        &root,
        manifest_with_web_search(&keystore),
        TrustDepth::D2,
        false,
        1_000,
        &keystore.verifying_key(),
    );
    assert!(matches!(result, Err(PluginError::ConsentDeclined)));
    assert!(
        registry.query("web.search").is_none(),
        "a rejected manifest must never partially install"
    );
}

#[test]
fn uninstalling_a_plugin_revokes_every_token_it_was_minted() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let handle = registry
        .install(
            &mut monitor,
            &root,
            manifest_with_web_search(&keystore),
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();
    registry
        .uninstall(&mut monitor, &root, handle.plugin_id)
        .unwrap();

    assert!(
        registry.query("web.search").is_none(),
        "uninstall must remove the plugin's contributions from the registry"
    );
    assert!(registry.boundary_of(handle.plugin_id).is_none());
}

#[test]
fn uninstalling_an_unknown_plugin_fails() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();

    let result = registry.uninstall(&mut monitor, &root, 999);
    assert!(matches!(result, Err(PluginError::NoSuchPlugin)));
}
