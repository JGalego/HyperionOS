//! docs/06 §Architecture's "device/session state" signal collector.
//! `hyperion-context`'s own doc comment names this as blocked on
//! `hyperion-device` persisting its `DeviceObject`s as real Knowledge
//! Graph nodes first -- which it now does (`DeviceRegistry::kg_node_for`)
//! -- and says "the same `intent_id`-as-anchor pattern... extends
//! naturally" once that's true. It does: `hyperion_context::Scope::anchors`
//! is already a generic `Vec<NodeId>`, so a real device's real KG node
//! composes as a context anchor with no code change needed on either
//! side. This proves that composition genuinely works end to end, not
//! just that both halves independently compile.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::{Budget, ContextEngine, Scope};
use hyperion_device::{
    CapabilityManifestEntry, DeviceRegistry, DeviceType, Direction, SafetyClass,
};
use hyperion_knowledge_graph::{EdgeOrigin, KnowledgeGraph};

#[test]
fn a_devices_real_kg_node_anchors_a_real_context_assembly() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let devices = DeviceRegistry::new(graph.clone());

    let device_id = devices
        .register(
            &monitor,
            &token,
            DeviceType::Mobile,
            "Acme",
            "Handset-1",
            vec![CapabilityManifestEntry {
                capability_name: "notify.show".to_string(),
                direction: Direction::Render,
                safety_class: SafetyClass::Cosmetic,
            }],
            1,
            0,
        )
        .unwrap();
    let device_node = devices
        .kg_node_for(device_id)
        .expect("register must have persisted a real Knowledge Graph node");

    // Something the user was just doing on this device -- linked from the
    // device's own node, exactly like an Intent's recent history would be.
    let recent_note = graph
        .put_node(
            &monitor,
            &token,
            None,
            "note",
            None,
            serde_json::json!({"text": "reply to Sam"}),
        )
        .unwrap();
    graph
        .link(
            &monitor,
            &token,
            device_node,
            "recently_active_on",
            recent_note,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "device_activity",
            None,
        )
        .unwrap();
    // An object with no relation to the device at all -- must not ride
    // along just because a device happens to exist.
    let unrelated = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            serde_json::json!({"title": "unrelated notes"}),
        )
        .unwrap();

    let engine = ContextEngine::new(graph);
    let scope = Scope {
        intent_id: "intent-1".to_string(),
        session_id: "session-1".to_string(),
        mentions: Vec::new(),
        anchors: vec![device_node],
    };
    let bundle = engine
        .assemble(&monitor, &token, &scope, Budget::default())
        .unwrap();

    let device_entry = bundle
        .entries
        .iter()
        .find(|e| e.node_id == device_node)
        .expect("the device's own node must be included as a real anchor");
    assert!(device_entry.source_signal.contains(&"anchor".to_string()));

    let note_entry = bundle
        .entries
        .iter()
        .find(|e| e.node_id == recent_note)
        .expect("a node linked from the device anchor must be reachable by traversal");
    assert!(note_entry
        .source_signal
        .contains(&"graph_traversal".to_string()));

    assert!(
        !bundle.entries.iter().any(|e| e.node_id == unrelated),
        "an object with no relation to the device must not be pulled in"
    );
}
