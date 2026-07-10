//! docs/20-device-framework.md's tiered pairing, manifest-contract
//! validation, the transient-connectivity state machine (§5.6), and the
//! car-loses-connectivity-mid-navigation substitute handoff (§10).

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_device::{
    CapabilityManifestEntry, DeviceError, DeviceRegistry, DeviceType, Direction, PresenceState,
    SafetyClass, TrustTier,
};
use hyperion_knowledge_graph::KnowledgeGraph;

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    DeviceRegistry,
    Arc<KnowledgeGraph>,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let registry = DeviceRegistry::new(graph.clone());
    (dir, monitor, token, registry, graph)
}

fn display_manifest() -> Vec<CapabilityManifestEntry> {
    vec![CapabilityManifestEntry {
        capability_name: "display.render".to_string(),
        direction: Direction::Render,
        safety_class: SafetyClass::Cosmetic,
    }]
}

#[test]
fn register_persists_the_device_as_a_real_knowledge_graph_node() {
    let (_dir, monitor, token, registry, graph) = setup();
    let device = registry
        .register(
            &monitor,
            &token,
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
}

#[test]
fn actuation_tier_pairing_requires_explicit_confirmation() {
    let (_dir, monitor, token, registry, _graph) = setup();
    let robot = registry
        .register(
            &monitor,
            &token,
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
    let (_dir, monitor, token, registry, _graph) = setup();
    let device = registry
        .register(
            &monitor,
            &token,
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
    let (_dir, monitor, token, registry, _graph) = setup();
    let display = registry
        .register(
            &monitor,
            &token,
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
    let (_dir, monitor, token, registry, _graph) = setup();
    let device = registry
        .register(
            &monitor,
            &token,
            DeviceType::Mobile,
            "Acme",
            "Phone-1",
            vec![],
            1,
            0,
        )
        .unwrap();

    registry.tick(5, 10, 30);
    assert_eq!(
        registry.get(device).unwrap().presence,
        PresenceState::Connected
    );

    registry.tick(20, 10, 30);
    assert_eq!(
        registry.get(device).unwrap().presence,
        PresenceState::Degraded
    );

    registry.tick(40, 10, 30);
    assert_eq!(
        registry.get(device).unwrap().presence,
        PresenceState::Disconnected
    );

    registry.heartbeat(device, 41).unwrap();
    assert_eq!(
        registry.get(device).unwrap().presence,
        PresenceState::Connected
    );
}

#[test]
fn a_disconnected_device_refuses_invocation_and_a_substitute_is_found() {
    let (_dir, monitor, token, registry, _graph) = setup();
    let nav_capability = "car.navigation.set_destination";
    let car = registry
        .register(
            &monitor,
            &token,
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
    let phone = registry
        .register(
            &monitor,
            &token,
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
    registry.tick(1000, 10, 30);
    registry.heartbeat(phone, 1000).unwrap();
    let result = registry.invoke(&monitor, &token, car, nav_capability, serde_json::json!({}));
    assert!(matches!(result, Err(DeviceError::Unreachable)));

    let substitute = registry.find_substitute(nav_capability, 1, car).unwrap();
    assert_eq!(substitute, phone);
}
