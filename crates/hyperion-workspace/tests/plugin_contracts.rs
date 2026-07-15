//! docs/998-roadmap.md's Resourceful pillar: a plugin-contributed `Contribution::UiComponent`
//! is a real, live UI-component registry — `known_contract_for` really finds it and converts
//! it into a real `CapabilityUiContract`.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{
    sign, CapabilityGrantRequest, Contribution, Operation, PluginManifest, PluginRegistry,
    QuarantineReason, TrustDepth, UiComponentContribution, UiRegionAffinity,
};
use hyperion_workspace::known_contract_for;

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

fn sample_ui_component() -> UiComponentContribution {
    UiComponentContribution {
        capability_ref: "weather.forecast".to_string(),
        panel_template: "weather.card".to_string(),
        region_affinity: UiRegionAffinity::Center,
        min_size: (200, 100),
        priority: 1.0,
        binds_category: Some("weather".to_string()),
        accessible_role: Some("region".to_string()),
        label_template: Some("Weather forecast".to_string()),
        keyboard_operations: vec!["Tab".to_string()],
        alt_text_hook: None,
        contrast_ratio: 4.5,
        has_motion: false,
        reduced_motion_alternative: false,
        language_tag: "en-US".to_string(),
        emits_audio: false,
        has_visual_alert_equivalent: true,
    }
}

fn install_ui_component(registry: &PluginRegistry, plugin_id: u64) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let (_dir, keystore) = keystore();

    let mut manifest = PluginManifest {
        plugin_id,
        publisher: "acme-ui".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::UiComponent(sample_ui_component())],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::Read,
            scope: "ui-component".to_string(),
            justification: "descriptive layout metadata only".to_string(),
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
fn an_unknown_capability_has_no_known_contract() {
    let registry = PluginRegistry::new();
    assert!(known_contract_for(&registry, "weather.forecast").is_none());
}

#[test]
fn a_plugin_contributed_component_is_found_and_correctly_converted() {
    let registry = PluginRegistry::new();
    install_ui_component(&registry, 1);

    let contract = known_contract_for(&registry, "weather.forecast")
        .expect("a real, installed UI component must be found");

    assert_eq!(contract.panel_template, "weather.card");
    assert_eq!(contract.min_size, (200, 100));
    assert_eq!(
        contract.region_affinity,
        hyperion_workspace::RegionAffinity::Center
    );
    assert_eq!(contract.label_template.as_deref(), Some("Weather forecast"));
    assert!(contract.variants.is_empty());
}

#[test]
fn a_network_egress_request_is_never_justified_by_a_ui_component_alone() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-ui".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::UiComponent(sample_ui_component())],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::NetworkEgress,
            scope: "ui-component".to_string(),
            justification: "a UI template alone can't justify this".to_string(),
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
fn quarantining_and_uninstalling_removes_it_from_lookup() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    install_ui_component(&registry, 1);
    assert!(known_contract_for(&registry, "weather.forecast").is_some());

    registry
        .quarantine(1, QuarantineReason::PolicyViolation)
        .unwrap();
    assert!(known_contract_for(&registry, "weather.forecast").is_none());

    registry.uninstall(&mut monitor, &root, 1).unwrap();
    assert!(known_contract_for(&registry, "weather.forecast").is_none());
}
