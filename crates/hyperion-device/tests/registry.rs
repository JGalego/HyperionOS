//! docs/20-device-framework.md's tiered pairing, manifest-contract
//! validation, the transient-connectivity state machine (§5.6), and the
//! car-loses-connectivity-mid-navigation substitute handoff (§10).

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_device::{
    CapabilityManifestEntry, DeviceError, DeviceRegistry, DeviceType, Direction, PresenceState,
    SafetyClass, TrustTier,
};
use hyperion_knowledge_graph::KnowledgeGraph;

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    CapabilityToken,
    DeviceRegistry,
    Arc<KnowledgeGraph>,
    Keystore,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let registry = DeviceRegistry::new(graph.clone());
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, monitor, token, registry, graph, keystore)
}

fn display_manifest() -> Vec<CapabilityManifestEntry> {
    vec![CapabilityManifestEntry {
        capability_name: "display.render".to_string(),
        direction: Direction::Render,
        safety_class: SafetyClass::Cosmetic,
    }]
}

/// docs/20 §8's device-impersonation defense, now real: every registration
/// needs a genuine Ed25519 signature over its own manifest fields — this
/// helper signs with `keystore` and registers in one step so the tests
/// below stay focused on what they actually exercise.
#[allow(clippy::too_many_arguments)]
fn register_device(
    registry: &DeviceRegistry,
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    keystore: &Keystore,
    device_type: DeviceType,
    manufacturer: &str,
    model: &str,
    capability_manifest: Vec<CapabilityManifestEntry>,
    owner: u64,
    now: u64,
) -> Result<u64, DeviceError> {
    let signature = hyperion_device::sign(
        device_type,
        manufacturer,
        model,
        &capability_manifest,
        owner,
        keystore,
    );
    registry.register(
        monitor,
        token,
        device_type,
        manufacturer,
        model,
        capability_manifest,
        owner,
        now,
        &signature,
        &keystore.verifying_key(),
    )
}

#[test]
fn register_persists_the_device_as_a_real_knowledge_graph_node() {
    let (_dir, monitor, token, registry, graph, keystore) = setup();
    let device = register_device(
        &registry,
        &monitor,
        &token,
        &keystore,
        DeviceType::Display,
        "Acme",
        "Display-1",
        display_manifest(),
        1,
        0,
    )
    .unwrap();

    let node_id = registry
        .kg_node_for(device)
        .expect("register must persist a real Knowledge Graph node");

    let node = graph.get(&monitor, &token, node_id).unwrap();
    assert_eq!(node.object_type, "device");
    assert_eq!(node.metadata["device_id"], device);
    assert_eq!(node.metadata["manufacturer"], "Acme");
    assert_eq!(node.metadata["model"], "Display-1");
    assert_eq!(
        node.metadata["pairing"],
        serde_json::Value::Null,
        "a freshly registered device has no pairing yet"
    );
}

#[test]
fn heartbeat_tick_pair_and_revoke_all_really_resync_the_kg_node() {
    let (_dir, monitor, token, registry, graph, keystore) = setup();
    let device = register_device(
        &registry,
        &monitor,
        &token,
        &keystore,
        DeviceType::Mobile,
        "Acme",
        "Phone-1",
        vec![CapabilityManifestEntry {
            capability_name: "phone.notify".to_string(),
            direction: Direction::Render,
            safety_class: SafetyClass::Cosmetic,
        }],
        1,
        0,
    )
    .unwrap();
    let node_id = registry.kg_node_for(device).unwrap();

    // heartbeat -- real, updated last_heartbeat really lands on the same real node.
    registry.heartbeat(&monitor, &token, device, 41).unwrap();
    let node = graph.get(&monitor, &token, node_id).unwrap();
    assert_eq!(node.metadata["last_heartbeat"], 41);

    // tick -- a real presence change really lands on the same real node too.
    registry.tick(&monitor, &token, 100, 10, 30).unwrap();
    let node = graph.get(&monitor, &token, node_id).unwrap();
    assert_eq!(node.metadata["presence"], "Disconnected");

    // pair -- the real grant now really appears on the node's own `pairing` field.
    registry
        .pair(
            &monitor,
            &token,
            device,
            TrustTier::View,
            vec!["phone.notify".to_string()],
            false,
        )
        .unwrap();
    let node = graph.get(&monitor, &token, node_id).unwrap();
    assert_eq!(
        node.metadata["pairing"]["granted_capabilities"],
        serde_json::json!(["phone.notify"])
    );

    // revoke -- the real grant is really gone from the node afterward, not left stale.
    registry.revoke(&monitor, &token, device).unwrap();
    let node = graph.get(&monitor, &token, node_id).unwrap();
    assert_eq!(node.metadata["pairing"], serde_json::Value::Null);

    // Every re-sync updates the same real node in place -- never a second, parallel one.
    assert_eq!(registry.kg_node_for(device), Some(node_id));
}

#[test]
fn actuation_tier_pairing_requires_explicit_confirmation() {
    let (_dir, monitor, token, registry, _graph, keystore) = setup();
    let robot = register_device(
        &registry,
        &monitor,
        &token,
        &keystore,
        DeviceType::Robot,
        "Acme",
        "Arm-1",
        vec![CapabilityManifestEntry {
            capability_name: "robot.arm.move".to_string(),
            direction: Direction::Actuate,
            safety_class: SafetyClass::High,
        }],
        1,
        0,
    )
    .unwrap();

    let denied = registry.pair(
        &monitor,
        &token,
        robot,
        TrustTier::Actuate,
        vec!["robot.arm.move".to_string()],
        false,
    );
    assert!(matches!(
        denied,
        Err(DeviceError::ActuationRequiresConfirmation)
    ));

    let ok = registry.pair(
        &monitor,
        &token,
        robot,
        TrustTier::Actuate,
        vec!["robot.arm.move".to_string()],
        true,
    );
    assert!(ok.is_ok());
}

#[test]
fn a_sense_tier_pairing_cannot_be_used_to_invoke_an_actuate_capability() {
    let (_dir, monitor, token, registry, _graph, keystore) = setup();
    let device = register_device(
        &registry,
        &monitor,
        &token,
        &keystore,
        DeviceType::HomeAppliance,
        "Acme",
        "Lock-1",
        vec![
            CapabilityManifestEntry {
                capability_name: "lock.status".to_string(),
                direction: Direction::Sense,
                safety_class: SafetyClass::Standard,
            },
            CapabilityManifestEntry {
                capability_name: "lock.actuate".to_string(),
                direction: Direction::Actuate,
                safety_class: SafetyClass::High,
            },
        ],
        1,
        0,
    )
    .unwrap();

    let result = registry.pair(
        &monitor,
        &token,
        device,
        TrustTier::Sense,
        vec!["lock.actuate".to_string()],
        false,
    );
    assert!(matches!(result, Err(DeviceError::InsufficientTier)));
}

#[test]
fn invoking_an_undeclared_or_unpaired_capability_is_denied() {
    let (_dir, monitor, token, registry, _graph, keystore) = setup();
    let display = register_device(
        &registry,
        &monitor,
        &token,
        &keystore,
        DeviceType::Display,
        "Acme",
        "Display-1",
        display_manifest(),
        1,
        0,
    )
    .unwrap();

    let unpaired = registry.invoke(
        &monitor,
        &token,
        display,
        "display.render",
        serde_json::json!({}),
    );
    assert!(matches!(unpaired, Err(DeviceError::NotPaired)));

    registry
        .pair(
            &monitor,
            &token,
            display,
            TrustTier::View,
            vec!["display.render".to_string()],
            false,
        )
        .unwrap();
    let undeclared = registry.invoke(
        &monitor,
        &token,
        display,
        "display.self_destruct",
        serde_json::json!({}),
    );
    assert!(matches!(
        undeclared,
        Err(DeviceError::CapabilityNotDeclared)
    ));

    let ok = registry.invoke(
        &monitor,
        &token,
        display,
        "display.render",
        serde_json::json!({"text": "hi"}),
    );
    assert!(ok.is_ok());
}

#[test]
fn presence_degrades_then_disconnects_and_recovers_on_heartbeat() {
    let (_dir, monitor, token, registry, _graph, keystore) = setup();
    let device = register_device(
        &registry,
        &monitor,
        &token,
        &keystore,
        DeviceType::Mobile,
        "Acme",
        "Phone-1",
        vec![],
        1,
        0,
    )
    .unwrap();

    registry.tick(&monitor, &token, 5, 10, 30).unwrap();
    assert_eq!(
        registry.get(device).unwrap().presence,
        PresenceState::Connected
    );

    registry.tick(&monitor, &token, 20, 10, 30).unwrap();
    assert_eq!(
        registry.get(device).unwrap().presence,
        PresenceState::Degraded
    );

    registry.tick(&monitor, &token, 40, 10, 30).unwrap();
    assert_eq!(
        registry.get(device).unwrap().presence,
        PresenceState::Disconnected
    );

    registry.heartbeat(&monitor, &token, device, 41).unwrap();
    assert_eq!(
        registry.get(device).unwrap().presence,
        PresenceState::Connected
    );
}

#[test]
fn a_disconnected_device_refuses_invocation_and_a_substitute_is_found() {
    let (_dir, monitor, token, registry, _graph, keystore) = setup();
    let nav_capability = "car.navigation.set_destination";
    let car = register_device(
        &registry,
        &monitor,
        &token,
        &keystore,
        DeviceType::Vehicle,
        "Acme",
        "Car-1",
        vec![CapabilityManifestEntry {
            capability_name: nav_capability.to_string(),
            direction: Direction::Render,
            safety_class: SafetyClass::Standard,
        }],
        1,
        0,
    )
    .unwrap();
    let phone = register_device(
        &registry,
        &monitor,
        &token,
        &keystore,
        DeviceType::Mobile,
        "Acme",
        "Phone-1",
        vec![CapabilityManifestEntry {
            capability_name: nav_capability.to_string(),
            direction: Direction::Render,
            safety_class: SafetyClass::Standard,
        }],
        1,
        0,
    )
    .unwrap();
    registry
        .pair(
            &monitor,
            &token,
            car,
            TrustTier::View,
            vec![nav_capability.to_string()],
            false,
        )
        .unwrap();

    // The car loses connectivity mid-navigation; the phone keeps sending
    // heartbeats throughout.
    registry.tick(&monitor, &token, 1000, 10, 30).unwrap();
    registry.heartbeat(&monitor, &token, phone, 1000).unwrap();
    let result = registry.invoke(&monitor, &token, car, nav_capability, serde_json::json!({}));
    assert!(matches!(result, Err(DeviceError::Unreachable)));

    let substitute = registry.find_substitute(nav_capability, 1, car).unwrap();
    assert_eq!(substitute, phone);
}
