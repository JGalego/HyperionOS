//! docs/24's own named "verify against publisher's registered key" gap, closed for real:
//! `PluginRegistry::install_with_publisher_registry`/`update_with_publisher_registry` resolve a
//! manifest's real trusted key from its own declared `publisher` via a real
//! `hyperion_crypto::PublisherRegistry`, instead of taking one caller-supplied key on faith.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::{Keystore, PublisherRegistry};
use hyperion_plugin_framework::{
    sign, CapabilityManifest, Contribution, ImplementationKind, PluginError, PluginManifest,
    PluginRegistry, SemanticContract, SideEffect, TrustDepth,
};

fn manifest_for(publisher: &str, plugin_id: u64, keystore: &Keystore) -> PluginManifest {
    let mut manifest = PluginManifest {
        plugin_id,
        publisher: publisher.to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: format!("{publisher}.capability"),
            contract: SemanticContract {
                inputs: vec!["text".to_string()],
                outputs: vec!["summary".to_string()],
                side_effects: vec![SideEffect::None],
            },
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.5,
            version: 1,
            native_binary: None,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = Some(sign(&manifest, keystore));
    manifest
}

#[test]
fn a_manifest_signed_by_its_own_declared_publishers_real_key_installs() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let acme_keystore = Keystore::ephemeral();

    let mut publishers = PublisherRegistry::new();
    publishers.register("acme-plugins", acme_keystore.verifying_key());

    let manifest = manifest_for("acme-plugins", 1, &acme_keystore);
    let handle = registry
        .install_with_publisher_registry(
            &mut monitor,
            &root,
            manifest,
            TrustDepth::D2,
            true,
            1_000,
            &publishers,
        )
        .unwrap();

    assert!(registry.query("acme-plugins.capability").is_some());
    assert!(registry.boundary_of(handle.plugin_id).is_some());
}

#[test]
fn a_manifest_claiming_a_publisher_it_wasnt_really_signed_by_is_rejected() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let acme_keystore = Keystore::ephemeral();
    let impostor_keystore = Keystore::ephemeral();

    let mut publishers = PublisherRegistry::new();
    publishers.register("acme-plugins", acme_keystore.verifying_key());
    publishers.register("impostor-plugins", impostor_keystore.verifying_key());

    // Declares "acme-plugins" as its publisher, but is really signed by a different real key.
    let manifest = manifest_for("acme-plugins", 1, &impostor_keystore);
    let result = registry.install_with_publisher_registry(
        &mut monitor,
        &root,
        manifest,
        TrustDepth::D2,
        true,
        1_000,
        &publishers,
    );

    assert!(
        matches!(result, Err(PluginError::SignatureInvalid)),
        "got: {result:?}"
    );
    assert!(registry.query("acme-plugins.capability").is_none());
}

#[test]
fn a_manifest_from_an_unregistered_publisher_is_rejected() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let keystore = Keystore::ephemeral();
    let publishers = PublisherRegistry::new(); // nobody registered

    let manifest = manifest_for("acme-plugins", 1, &keystore);
    let result = registry.install_with_publisher_registry(
        &mut monitor,
        &root,
        manifest,
        TrustDepth::D2,
        true,
        1_000,
        &publishers,
    );

    assert!(
        matches!(&result, Err(PluginError::UnknownPublisher(publisher)) if publisher == "acme-plugins"),
        "got: {result:?}"
    );
}

#[test]
fn two_different_publishers_each_verify_against_their_own_real_key() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let acme_keystore = Keystore::ephemeral();
    let globex_keystore = Keystore::ephemeral();

    let mut publishers = PublisherRegistry::new();
    publishers.register("acme-plugins", acme_keystore.verifying_key());
    publishers.register("globex-plugins", globex_keystore.verifying_key());

    registry
        .install_with_publisher_registry(
            &mut monitor,
            &root,
            manifest_for("acme-plugins", 1, &acme_keystore),
            TrustDepth::D2,
            true,
            1_000,
            &publishers,
        )
        .unwrap();
    registry
        .install_with_publisher_registry(
            &mut monitor,
            &root,
            manifest_for("globex-plugins", 2, &globex_keystore),
            TrustDepth::D2,
            true,
            1_001,
            &publishers,
        )
        .unwrap();

    assert!(registry.query("acme-plugins.capability").is_some());
    assert!(registry.query("globex-plugins.capability").is_some());
}

#[test]
fn update_with_publisher_registry_also_resolves_the_real_trusted_key() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let acme_keystore = Keystore::ephemeral();

    let mut publishers = PublisherRegistry::new();
    publishers.register("acme-plugins", acme_keystore.verifying_key());

    let handle = registry
        .install_with_publisher_registry(
            &mut monitor,
            &root,
            manifest_for("acme-plugins", 1, &acme_keystore),
            TrustDepth::D2,
            true,
            1_000,
            &publishers,
        )
        .unwrap();

    let mut updated = manifest_for("acme-plugins", 1, &acme_keystore);
    updated.sdk_version = 2;
    updated.signature = Some(sign(&updated, &acme_keystore));

    let new_grants = registry
        .update_with_publisher_registry(
            &mut monitor,
            &root,
            handle.plugin_id,
            updated,
            TrustDepth::D2,
            true,
            &publishers,
        )
        .unwrap();
    assert!(new_grants.is_empty());
}
