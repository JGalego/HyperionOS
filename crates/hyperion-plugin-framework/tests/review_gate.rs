//! docs/24 §5's review gate: an over-requested permission is rejected
//! pre-consent, a tampered signature is caught, and a `capability_id`
//! collision is either a real competing implementation (structurally
//! compatible) or a real, distinct `version_variant()` registry entry
//! (structurally incompatible) — never a silent shadow, and never an
//! outright install failure either.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{
    sign, validate_manifest, CapabilityGrantRequest, CapabilityManifest, Contribution,
    ImplementationKind, Operation, PluginError, PluginManifest, PluginRegistry, PrivacyTier,
    SemanticContract, SideEffect, TrustDepth,
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
            native_binary: None,
            privacy_tier: PrivacyTier::Local,
            resource_profile: None,
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
    let Contribution::Capability(cm) = &mut manifest.contributions[0] else {
        unreachable!("test fixture always installs a Capability contribution")
    };
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
fn a_structurally_incompatible_collision_installs_under_a_real_version_variant_id() {
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
    let Contribution::Capability(cm) = &mut incompatible.contributions[0] else {
        unreachable!("test fixture always installs a Capability contribution")
    };
    cm.contract.outputs = vec!["summary".to_string(), "extra_field".to_string()];
    incompatible.signature = Some(sign(&incompatible, &keystore));

    // docs/24 §5's `version_variant()`: an incompatible collision installs in full, under a
    // real, distinct id -- never an outright install failure.
    registry
        .install(
            &mut monitor,
            &root,
            incompatible,
            TrustDepth::D0,
            true,
            1_001,
            &keystore.verifying_key(),
        )
        .unwrap();

    let original = registry.query("document.summarize").unwrap();
    assert_eq!(
        original.implementations.len(),
        1,
        "the original entry must be untouched by an incompatible collision, not merged into it"
    );

    let variant = registry
        .query("document.summarize#2")
        .expect("an incompatible collision must register under a real, discoverable variant id");
    assert_eq!(variant.implementations.len(), 1);
    assert_eq!(variant.contract.outputs, vec!["summary", "extra_field"]);
    assert_ne!(
        variant.contract, original.contract,
        "the variant keeps its own real, incompatible contract, not a copy of the original's"
    );
}

#[test]
fn a_second_incompatible_collision_gets_its_own_distinct_variant_id() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    for (plugin_id, extra_output, now) in [
        (1, None, 1_000),
        (2, Some("extra_field"), 1_001),
        (3, Some("another_field"), 1_002),
    ] {
        let mut manifest = base_manifest();
        manifest.plugin_id = plugin_id;
        if let Some(extra) = extra_output {
            let Contribution::Capability(cm) = &mut manifest.contributions[0] else {
                unreachable!("test fixture always installs a Capability contribution")
            };
            cm.contract.outputs = vec!["summary".to_string(), extra.to_string()];
        }
        manifest.signature = Some(sign(&manifest, &keystore));
        registry
            .install(
                &mut monitor,
                &root,
                manifest,
                TrustDepth::D0,
                true,
                now,
                &keystore.verifying_key(),
            )
            .unwrap();
    }

    assert!(registry.query("document.summarize#2").is_some());
    assert!(
        registry.query("document.summarize#3").is_some(),
        "a second, independently-incompatible collision must not collide with the first variant"
    );
}
