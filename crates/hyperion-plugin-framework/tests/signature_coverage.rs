//! What a manifest's signature actually commits to.
//!
//! A signature that covers less than the manifest *means* is one an attacker can reuse. These
//! tests take a legitimately signed manifest, change one security-relevant thing, and require that
//! it stops verifying -- because for most of these fields, it previously did not.

use std::path::PathBuf;

use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{
    sign, CapabilityGrantRequest, CapabilityManifest, Contribution, ImplementationKind,
    NativeBinaryDescriptor, Operation, PluginManifest, PrivacyTier, SemanticContract, SideEffect,
    TrustDepth,
};

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

fn signed_manifest(keystore: &Keystore) -> PluginManifest {
    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "app.tally".to_string(),
            contract: SemanticContract {
                inputs: vec!["hyperion-app/v3|app|tally|alice|stateless|adds up".to_string()],
                outputs: vec!["result".to_string()],
                side_effects: vec![SideEffect::None],
            },
            implementation_kind: ImplementationKind::NativeBinary,
            quality_score: 1.0,
            version: 1,
            native_binary: Some(NativeBinaryDescriptor {
                program: PathBuf::from("/usr/bin/tally"),
                args: vec!["--quiet".to_string()],
                script: Some(PathBuf::from("/apps/tally/script")),
            }),
            privacy_tier: PrivacyTier::Local,
            resource_profile: None,
        })],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::Execute,
            scope: "app.tally".to_string(),
            justification: "adds up invoices".to_string(),
        }],
        min_trust_depth: TrustDepth::D2,
    };
    manifest.signature = Some(sign(&manifest, keystore));
    manifest
}

/// `validate_manifest`'s signature check, reached the way `install` reaches it.
fn verifies(manifest: &PluginManifest, keystore: &Keystore) -> bool {
    !matches!(
        hyperion_plugin_framework::validate_manifest(manifest, &keystore.verifying_key()),
        Err(hyperion_plugin_framework::PluginError::SignatureInvalid)
    )
}

fn capability(manifest: &mut PluginManifest) -> &mut CapabilityManifest {
    match &mut manifest.contributions[0] {
        Contribution::Capability(cm) => cm,
        _ => unreachable!("this fixture's first contribution is a Capability"),
    }
}

#[test]
fn an_untouched_manifest_verifies() {
    let (_dir, keystore) = keystore();
    assert!(verifies(&signed_manifest(&keystore), &keystore));
}

#[test]
fn swapping_the_program_that_will_run_breaks_the_signature() {
    // The most consequential field of all, and the one the old encoding left entirely uncovered: a
    // legitimately signed manifest could be pointed at a different executable and still verify.
    let (_dir, keystore) = keystore();
    let mut manifest = signed_manifest(&keystore);
    capability(&mut manifest)
        .native_binary
        .as_mut()
        .unwrap()
        .program = PathBuf::from("/usr/bin/something-else");
    assert!(!verifies(&manifest, &keystore));
}

#[test]
fn swapping_the_script_or_its_arguments_breaks_the_signature() {
    let (_dir, keystore) = keystore();

    let mut swapped_script = signed_manifest(&keystore);
    capability(&mut swapped_script)
        .native_binary
        .as_mut()
        .unwrap()
        .script = Some(PathBuf::from("/apps/other/script"));
    assert!(!verifies(&swapped_script, &keystore));

    let mut swapped_args = signed_manifest(&keystore);
    capability(&mut swapped_args)
        .native_binary
        .as_mut()
        .unwrap()
        .args = vec!["--not-quiet".to_string()];
    assert!(!verifies(&swapped_args, &keystore));
}

#[test]
fn widening_a_requested_permission_breaks_the_signature() {
    // Unsigned, permissions could be widened on a manifest whose signature still verified, which
    // is the review gate defeated rather than merely bypassed.
    let (_dir, keystore) = keystore();

    let mut escalated = signed_manifest(&keystore);
    escalated.requested_permissions[0].operation = Operation::Write;
    assert!(!verifies(&escalated, &keystore));

    let mut extra = signed_manifest(&keystore);
    extra.requested_permissions.push(CapabilityGrantRequest {
        operation: Operation::NetworkEgress,
        scope: "app.tally".to_string(),
        justification: "smuggled".to_string(),
    });
    assert!(!verifies(&extra, &keystore));
}

#[test]
fn rewriting_the_declared_side_effects_breaks_the_signature() {
    // Side effects are what the review gate reasons about, and what `hyperion-app` reads to decide
    // whether an app is granted durable storage.
    let (_dir, keystore) = keystore();
    let mut manifest = signed_manifest(&keystore);
    capability(&mut manifest).contract.side_effects = vec![SideEffect::CreatesSemanticObject];
    assert!(!verifies(&manifest, &keystore));
}

#[test]
fn rewriting_the_contract_breaks_the_signature() {
    // An app's owner and its durable-storage declaration ride inside these strings, so a signature
    // that did not cover them left both editable.
    let (_dir, keystore) = keystore();
    let mut manifest = signed_manifest(&keystore);
    capability(&mut manifest).contract.inputs =
        vec!["hyperion-app/v3|app|tally|mallory|keeps-data|adds up".to_string()];
    assert!(!verifies(&manifest, &keystore));
}

#[test]
fn lowering_the_minimum_trust_depth_breaks_the_signature() {
    // The depth decides how confined the process is. Unsigned, a manifest could ask for a weaker
    // sandbox than its publisher agreed to.
    let (_dir, keystore) = keystore();
    let mut manifest = signed_manifest(&keystore);
    manifest.min_trust_depth = TrustDepth::D0;
    assert!(!verifies(&manifest, &keystore));
}

#[test]
fn changing_the_implementation_kind_or_privacy_tier_breaks_the_signature() {
    let (_dir, keystore) = keystore();

    let mut kind = signed_manifest(&keystore);
    capability(&mut kind).implementation_kind = ImplementationKind::CloudApi;
    assert!(!verifies(&kind, &keystore));

    let mut tier = signed_manifest(&keystore);
    capability(&mut tier).privacy_tier = PrivacyTier::ConsentedCloud;
    assert!(!verifies(&tier, &keystore));
}

#[test]
fn two_manifests_differing_only_in_where_a_boundary_falls_do_not_share_a_signature() {
    // Plain concatenation is ambiguous: ("ab", "c") and ("a", "bc") produce identical bytes, so two
    // genuinely different manifests could share one signature. Length prefixes are what stop that.
    let (_dir, keystore) = keystore();

    let mut first = signed_manifest(&keystore);
    capability(&mut first).capability_id = "app.ta".to_string();
    first.signature = Some(sign(&first, &keystore));

    let mut second = first.clone();
    capability(&mut second).capability_id = "app.t".to_string();
    // Same signature bytes, different manifest: it must not verify.
    assert!(!verifies(&second, &keystore));
}
