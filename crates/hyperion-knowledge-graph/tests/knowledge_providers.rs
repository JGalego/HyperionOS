//! docs/998-roadmap.md's Resourceful pillar: a plugin-contributed `Contribution::KnowledgeProvider`
//! is a real, live (topic -> capability_id) registry — `capability_for_topic`/
//! `capabilities_for_topic` really find it, and it never bypasses the Capability Registry's own
//! dispatch/consent path for whatever capability it points at.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_knowledge_graph::{capabilities_for_topic, capability_for_topic};
use hyperion_plugin_framework::{
    sign, CapabilityGrantRequest, Contribution, KnowledgeProviderContribution, Operation,
    PluginManifest, PluginRegistry, QuarantineReason, TrustDepth,
};

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

fn install_provider(registry: &PluginRegistry, plugin_id: u64, topic: &str, capability_id: &str) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let (_dir, keystore) = keystore();

    let mut manifest = PluginManifest {
        plugin_id,
        publisher: "acme-knowledge".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::KnowledgeProvider(
            KnowledgeProviderContribution {
                topic: topic.to_string(),
                capability_id: capability_id.to_string(),
            },
        )],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::Read,
            scope: "knowledge-provider".to_string(),
            justification: "descriptive lookup entry only".to_string(),
        }],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = Some(sign(&manifest, &keystore));

    registry
        .install(
            &mut monitor,
            &root,
            manifest,
            TrustDepth::D0,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();
}

#[test]
fn an_unknown_topic_has_no_capability() {
    let registry = PluginRegistry::new();
    assert!(capability_for_topic(&registry, "weather").is_none());
    assert!(capabilities_for_topic(&registry, "weather").is_empty());
}

#[test]
fn a_plugin_contributed_provider_is_found_by_exact_topic() {
    let registry = PluginRegistry::new();
    install_provider(&registry, 1, "weather", "weather.forecast");

    assert_eq!(
        capability_for_topic(&registry, "weather"),
        Some("weather.forecast".to_string())
    );
    assert!(capability_for_topic(&registry, "stocks").is_none());
}

#[test]
fn two_providers_for_the_same_topic_are_both_returned() {
    let registry = PluginRegistry::new();
    install_provider(&registry, 1, "weather", "weather.forecast");
    install_provider(&registry, 2, "weather", "weather.alt-source");

    let mut capabilities = capabilities_for_topic(&registry, "weather");
    capabilities.sort();
    assert_eq!(
        capabilities,
        vec![
            "weather.alt-source".to_string(),
            "weather.forecast".to_string()
        ]
    );
}

#[test]
fn a_network_egress_request_is_never_justified_by_a_knowledge_provider_alone() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-knowledge".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::KnowledgeProvider(
            KnowledgeProviderContribution {
                topic: "weather".to_string(),
                capability_id: "weather.forecast".to_string(),
            },
        )],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::NetworkEgress,
            scope: "knowledge-provider".to_string(),
            justification: "a lookup entry alone can't justify this".to_string(),
        }],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = Some(sign(&manifest, &keystore));

    let result = registry.install(
        &mut monitor,
        &root,
        manifest,
        TrustDepth::D0,
        true,
        1_000,
        &keystore.verifying_key(),
    );
    assert!(result.is_err());
}

#[test]
fn quarantining_and_uninstalling_the_provider_plugin_removes_it_from_lookup() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    install_provider(&registry, 1, "weather", "weather.forecast");
    assert!(capability_for_topic(&registry, "weather").is_some());

    registry
        .quarantine(1, QuarantineReason::PolicyViolation)
        .unwrap();
    assert!(capability_for_topic(&registry, "weather").is_none());

    registry.uninstall(&mut monitor, &root, 1).unwrap();
    assert!(capability_for_topic(&registry, "weather").is_none());
}
