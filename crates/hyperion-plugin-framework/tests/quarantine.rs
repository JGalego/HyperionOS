//! docs/24 §6's `registry_quarantine`: disables a plugin's registry
//! entries without a full uninstall — its tokens remain live, but it's
//! never again returned as an eligible candidate.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{
    sign, CapabilityManifest, Contribution, ImplementationKind, PluginError, PluginManifest,
    PluginRegistry, PrivacyTier, QuarantineReason, SemanticContract, SideEffect, TrustDepth,
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
            resource_profile: None,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    m.signature = Some(sign(&m, keystore));
    m
}

#[test]
fn a_quarantined_plugins_capability_is_no_longer_returned_by_query() {
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
    assert!(registry.query("document.summarize").is_some());

    registry
        .quarantine(handle.plugin_id, QuarantineReason::PolicyViolation)
        .unwrap();

    assert!(
        registry.query("document.summarize").is_none(),
        "a quarantined entry must never be returned as an eligible candidate"
    );
}

#[test]
fn quarantining_an_unknown_plugin_fails() {
    let registry = PluginRegistry::new();
    let result = registry.quarantine(999, QuarantineReason::IntegrityFailure);
    assert!(matches!(result, Err(PluginError::NoSuchPlugin)));
}
