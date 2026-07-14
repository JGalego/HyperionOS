//! docs/20 §8's device-impersonation defense: `DeviceRegistry::register`
//! must refuse a manifest whose signature doesn't verify, whether because
//! it was signed by a different real key or because the manifest itself
//! was altered after signing.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_device::{
    CapabilityManifestEntry, DeviceError, DeviceRegistry, DeviceType, Direction, SafetyClass,
};
use hyperion_knowledge_graph::KnowledgeGraph;

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    DeviceRegistry,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let registry = DeviceRegistry::new(graph);
    (dir, monitor, token, registry)
}

fn manifest() -> Vec<CapabilityManifestEntry> {
    vec![CapabilityManifestEntry {
        capability_name: "display.render".to_string(),
        direction: Direction::Render,
        safety_class: SafetyClass::Cosmetic,
    }]
}

#[test]
fn a_manifest_signed_by_a_different_real_key_is_rejected() {
    let (dir, monitor, token, registry) = setup();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    let forger_dir = tempfile::tempdir().unwrap();
    let forger_keystore = Keystore::open_or_create(&forger_dir.path().join("forger.key")).unwrap();

    // Signed with a real key -- just not the one the registry is told to trust.
    let signature = hyperion_device::sign(
        DeviceType::Display,
        "Acme",
        "D1",
        &manifest(),
        1,
        &forger_keystore,
    );
    let result = registry.register(
        &monitor,
        &token,
        DeviceType::Display,
        "Acme",
        "D1",
        manifest(),
        1,
        0,
        &signature,
        &keystore.verifying_key(),
    );
    assert!(matches!(result, Err(DeviceError::SignatureInvalid)));
}

#[test]
fn a_manifest_field_changed_after_signing_is_rejected() {
    let (dir, monitor, token, registry) = setup();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();

    let signature =
        hyperion_device::sign(DeviceType::Display, "Acme", "D1", &manifest(), 1, &keystore);
    // The manifest presented to `register` no longer matches what was actually signed
    // (a real forger claiming a different model under a stolen signature).
    let result = registry.register(
        &monitor,
        &token,
        DeviceType::Display,
        "Acme",
        "D1-forged-model",
        manifest(),
        1,
        0,
        &signature,
        &keystore.verifying_key(),
    );
    assert!(matches!(result, Err(DeviceError::SignatureInvalid)));
}

#[test]
fn a_correctly_signed_manifest_registers_cleanly() {
    let (dir, monitor, token, registry) = setup();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    let signature =
        hyperion_device::sign(DeviceType::Display, "Acme", "D1", &manifest(), 1, &keystore);
    let result = registry.register(
        &monitor,
        &token,
        DeviceType::Display,
        "Acme",
        "D1",
        manifest(),
        1,
        0,
        &signature,
        &keystore.verifying_key(),
    );
    assert!(result.is_ok());
}
