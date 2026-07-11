//! docs/24 §5's review gate: an over-requested permission is rejected
//! pre-consent, a tampered signature is caught, and a `capability_id`
//! collision is either a real competing implementation or a hard
//! rejection — never a silent shadow.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{
    sign, validate_manifest, CapabilityGrantRequest, CapabilityManifest, Contribution,
    ImplementationKind, Operation, PluginError, PluginManifest, PluginRegistry, SemanticContract,
    SideEffect, TrustDepth,
};

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

fn base_manifest() -> PluginManifest {
    PluginManifest {
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
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    }
}

#[test]
fn requesting_network_egress_with_no_declared_side_effect_is_rejected() {
    let (_dir, keystore) = keystore();
    let mut manifest = base_manifest();
    manifest.requested_permissions = vec![CapabilityGrantRequest {
        operation: Operation::NetworkEgress,
        scope: "any".to_string(),
        justification: "phone home".to_string(),
    }];
    manifest.signature = Some(sign(&manifest, &keystore));

    let result = validate_manifest(&manifest, &keystore.verifying_key());
    assert!(matches!(
        result,
        Err(PluginError::PermissionOverreach(Operation::NetworkEgress))
    ));
}

#[test]
fn requesting_network_egress_with_a_declared_side_effect_is_accepted() {
    let (_dir, keystore) = keystore();
    let mut manifest = base_manifest();
    let Contribution::Capability(cm) = &mut manifest.contributions[0];
    cm.contract.side_effects = vec![SideEffect::NetworkEgress];
    manifest.requested_permissions = vec![CapabilityGrantRequest {
        operation: Operation::NetworkEgress,
        scope: "web.search".to_string(),
        justification: "fetch results".to_string(),
    }];
    manifest.signature = Some(sign(&manifest, &keystore));

    assert!(validate_manifest(&manifest, &keystore.verifying_key()).is_ok());
}

#[test]
fn a_read_permission_never_needs_a_declared_side_effect() {
    let (_dir, keystore) = keystore();
    let mut manifest = base_manifest();
    manifest.requested_permissions = vec![CapabilityGrantRequest {
        operation: Operation::Read,
        scope: "notes".to_string(),
        justification: "read notes".to_string(),
    }];
    manifest.signature = Some(sign(&manifest, &keystore));

    assert!(validate_manifest(&manifest, &keystore.verifying_key()).is_ok());
}

#[test]
fn a_tampered_manifest_fails_signature_verification() {
    let (_dir, keystore) = keystore();
    let mut manifest = base_manifest();
    manifest.signature = Some(sign(&manifest, &keystore));
    manifest.publisher = "not-acme-plugins".to_string(); // tampered after signing

    let result = validate_manifest(&manifest, &keystore.verifying_key());
    assert!(matches!(result, Err(PluginError::SignatureInvalid)));
}

#[test]
fn a_manifest_signed_by_an_untrusted_key_fails_verification() {
    let (_dir, real_signer) = keystore();
    let (_dir2, forger) = keystore();
    let mut manifest = base_manifest();
    manifest.signature = Some(sign(&manifest, &forger));

    let result = validate_manifest(&manifest, &real_signer.verifying_key());
    assert!(
        matches!(result, Err(PluginError::SignatureInvalid)),
        "a manifest signed by any real keypair other than the trusted device key must be \
         rejected -- unlike a checksum, which a forger could always recompute"
    );
}

#[test]
fn a_structurally_compatible_collision_becomes_a_second_competing_implementation() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let mut first = base_manifest();
    first.plugin_id = 1;
    first.signature = Some(sign(&first, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            first,
            TrustDepth::D0,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let mut second = base_manifest();
    second.plugin_id = 2;
    second.signature = Some(sign(&second, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            second,
            TrustDepth::D0,
            true,
            1_001,
            &keystore.verifying_key(),
        )
        .unwrap();

    let entry = registry.query("document.summarize").unwrap();
    assert_eq!(
        entry.implementations.len(),
        2,
        "two structurally-identical contracts must compete as candidates, not overwrite each other"
    );
}

#[test]
fn a_structurally_incompatible_collision_is_rejected() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let mut first = base_manifest();
    first.plugin_id = 1;
    first.signature = Some(sign(&first, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            first,
            TrustDepth::D0,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let mut incompatible = base_manifest();
    incompatible.plugin_id = 2;
    let Contribution::Capability(cm) = &mut incompatible.contributions[0];
    cm.contract.outputs = vec!["summary".to_string(), "extra_field".to_string()];
    incompatible.signature = Some(sign(&incompatible, &keystore));

    let result = registry.install(
        &mut monitor,
        &root,
        incompatible,
        TrustDepth::D0,
        true,
        1_001,
        &keystore.verifying_key(),
    );
    assert!(matches!(
        result,
        Err(PluginError::CapabilityCollisionIncompatible)
    ));
}
