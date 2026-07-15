//! docs/998-roadmap.md's Resourceful pillar: a plugin-contributed `Contribution::HardwareSupport`
//! is a real, live "device driver registry" — `known_capability_manifest` really finds it, and
//! `DeviceRegistry::register`'s own real signature requirement is completely untouched.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_device::{known_capability_manifest, DeviceType};
use hyperion_plugin_framework::{
    sign, CapabilityGrantRequest, Contribution, HardwareCapabilityEntry, HardwareDeviceType,
    HardwareDirection, HardwareSafetyClass, HardwareSupportContribution, Operation, PluginManifest,
    PluginRegistry, QuarantineReason, TrustDepth,
};

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

fn install_smart_bulb_driver(registry: &PluginRegistry, plugin_id: u64) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let (_dir, keystore) = keystore();

    let mut manifest = PluginManifest {
        plugin_id,
        publisher: "acme-drivers".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::HardwareSupport(HardwareSupportContribution {
            device_type: HardwareDeviceType::HomeAppliance,
            manufacturer: "Acme".to_string(),
            model: "SmartBulb-3000".to_string(),
            capability_manifest: vec![HardwareCapabilityEntry {
                capability_name: "light.toggle".to_string(),
                direction: HardwareDirection::Actuate,
                safety_class: HardwareSafetyClass::Cosmetic,
            }],
        })],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::Read,
            scope: "hardware-support".to_string(),
            justification: "descriptive driver metadata only".to_string(),
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
fn an_unknown_device_has_no_known_manifest() {
    let registry = PluginRegistry::new();
    assert!(known_capability_manifest(
        &registry,
        DeviceType::HomeAppliance,
        "Acme",
        "SmartBulb-3000"
    )
    .is_none());
}

#[test]
fn a_plugin_contributed_driver_is_found_by_exact_manufacturer_and_model() {
    let registry = PluginRegistry::new();
    install_smart_bulb_driver(&registry, 1);

    let manifest = known_capability_manifest(
        &registry,
        DeviceType::HomeAppliance,
        "Acme",
        "SmartBulb-3000",
    )
    .expect("a real, installed driver profile must be found");

    assert_eq!(manifest.len(), 1);
    assert_eq!(manifest[0].capability_name, "light.toggle");
}

#[test]
fn a_mismatched_model_never_matches() {
    let registry = PluginRegistry::new();
    install_smart_bulb_driver(&registry, 1);

    assert!(known_capability_manifest(
        &registry,
        DeviceType::HomeAppliance,
        "Acme",
        "SmartBulb-9999"
    )
    .is_none());
}

#[test]
fn a_network_egress_request_is_never_justified_by_hardware_support_alone() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-drivers".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::HardwareSupport(HardwareSupportContribution {
            device_type: HardwareDeviceType::HomeAppliance,
            manufacturer: "Acme".to_string(),
            model: "SmartBulb-3000".to_string(),
            capability_manifest: vec![],
        })],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::NetworkEgress,
            scope: "hardware-support".to_string(),
            justification: "a driver profile alone can't justify this".to_string(),
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
fn quarantining_the_driver_plugin_hides_it_from_lookup() {
    let registry = PluginRegistry::new();
    install_smart_bulb_driver(&registry, 1);
    assert!(known_capability_manifest(
        &registry,
        DeviceType::HomeAppliance,
        "Acme",
        "SmartBulb-3000"
    )
    .is_some());

    registry
        .quarantine(1, QuarantineReason::PolicyViolation)
        .unwrap();
    assert!(known_capability_manifest(
        &registry,
        DeviceType::HomeAppliance,
        "Acme",
        "SmartBulb-3000"
    )
    .is_none());
}

#[test]
fn uninstalling_the_driver_plugin_removes_it_from_lookup() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    install_smart_bulb_driver(&registry, 1);
    assert!(known_capability_manifest(
        &registry,
        DeviceType::HomeAppliance,
        "Acme",
        "SmartBulb-3000"
    )
    .is_some());

    registry.uninstall(&mut monitor, &root, 1).unwrap();
    assert!(known_capability_manifest(
        &registry,
        DeviceType::HomeAppliance,
        "Acme",
        "SmartBulb-3000"
    )
    .is_none());
}
