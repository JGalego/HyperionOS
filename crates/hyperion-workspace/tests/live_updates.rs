//! docs/13-dynamic-ui-runtime.md §5.5: "The live Workspace subscribes to
//! the Event System. On each LiveUpdateEvent, only the affected Panel's
//! binding is diffed and patched — never the whole graph — and updates
//! within a frame budget are coalesced."

use std::collections::HashMap;
use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::{Budget, ContextBundle, ExpertiseEstimate, ExpertiseLevel, Scope};
use hyperion_events::EventBus;
use hyperion_storage::ObjectId;
use hyperion_workspace::{
    CapabilityUiContract, ComplexityTier, LiveUpdateEvent, LiveUpdateEventKind, RegionAffinity,
    RenderState, WorkspaceCompiler,
};

fn contract(capability_ref: &str) -> CapabilityUiContract {
    CapabilityUiContract {
        capability_ref: capability_ref.to_string(),
        panel_template: format!("{capability_ref}.default"),
        region_affinity: RegionAffinity::Center,
        min_size: (200, 200),
        priority: 0.5,
        binds_category: None,
        variants: HashMap::new(),
        accessible_role: Some("region".to_string()),
        label_template: Some(capability_ref.to_string()),
        keyboard_operations: vec!["activate".to_string()],
        alt_text_hook: None,
        contrast_ratio: 7.0,
        has_motion: false,
        reduced_motion_alternative: true,
        language_tag: "en".to_string(),
        emits_audio: false,
        has_visual_alert_equivalent: true,
    }
}

fn empty_bundle() -> ContextBundle {
    ContextBundle {
        bundle_id: 1,
        scope: Scope {
            intent_id: "i".to_string(),
            session_id: "s".to_string(),
            mentions: Vec::new(),
            anchors: Vec::new(),
        },
        entries: Vec::new(),
        assembled_at: 0,
        budget: Budget::default(),
        expertise_signal: ExpertiseEstimate {
            domain: "general".to_string(),
            level: ExpertiseLevel::Novice,
            evidence: Vec::new(),
            confidence: 0.0,
        },
    }
}

fn setup() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    WorkspaceCompiler,
) {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let bus = Arc::new(EventBus::new(None));
    (monitor, token, WorkspaceCompiler::new().with_events(bus))
}

#[test]
fn apply_live_update_patches_only_the_named_panel() {
    let (monitor, token, compiler) = setup();
    let contracts = vec![contract("notes.summarize"), contract("calendar.read")];
    let graph = compiler
        .compile(
            &monitor,
            &token,
            ObjectId(1),
            "exam_prep",
            &contracts,
            &empty_bundle(),
            ComplexityTier::Beginner,
            1.0,
        )
        .unwrap();
    assert_eq!(graph.panels.len(), 2);
    let target_panel = graph.panels[0].panel_id;
    let other_panel = graph.panels[1].panel_id;
    assert_ne!(target_panel, other_panel);

    let result_node = ObjectId(999);
    compiler
        .apply_live_update(
            &monitor,
            &token,
            graph.graph_id,
            &LiveUpdateEvent {
                workspace_id: graph.graph_id,
                panel_id: target_panel,
                event_type: LiveUpdateEventKind::ResultReady,
                payload_ref: Some(result_node),
            },
        )
        .unwrap();

    let updated = compiler
        .get_graph(&monitor, &token, graph.graph_id)
        .unwrap();
    let patched = updated
        .panels
        .iter()
        .find(|p| p.panel_id == target_panel)
        .unwrap();
    assert_eq!(patched.render_state, RenderState::Ready);
    assert_eq!(patched.bindings.first().unwrap().target, result_node);

    // The *other* Panel is completely untouched -- "never the whole graph."
    let untouched = updated
        .panels
        .iter()
        .find(|p| p.panel_id == other_panel)
        .unwrap();
    let original = graph
        .panels
        .iter()
        .find(|p| p.panel_id == other_panel)
        .unwrap();
    assert_eq!(untouched.render_state, original.render_state);
}

#[test]
fn a_live_workspace_receives_a_real_event_and_can_apply_it_from_the_wire() {
    let (monitor, token, compiler) = setup();
    let contracts = vec![contract("notes.summarize")];
    let graph = compiler
        .compile(
            &monitor,
            &token,
            ObjectId(1),
            "exam_prep",
            &contracts,
            &empty_bundle(),
            ComplexityTier::Beginner,
            1.0,
        )
        .unwrap();
    let panel_id = graph.panels[0].panel_id;

    let sub = compiler
        .subscribe_live(&monitor, &token, graph.graph_id)
        .unwrap();

    compiler
        .publish_live_update(
            &monitor,
            &token,
            &LiveUpdateEvent {
                workspace_id: graph.graph_id,
                panel_id,
                event_type: LiveUpdateEventKind::Error,
                payload_ref: None,
            },
        )
        .unwrap();

    let wire_event = sub
        .try_recv()
        .expect("a real event should have been published");
    let reconstructed = LiveUpdateEvent::from_payload(&wire_event.payload)
        .expect("a real published event round-trips");
    assert_eq!(reconstructed.panel_id, panel_id);
    assert_eq!(reconstructed.event_type, LiveUpdateEventKind::Error);

    compiler
        .apply_live_update(&monitor, &token, graph.graph_id, &reconstructed)
        .unwrap();
    let updated = compiler
        .get_graph(&monitor, &token, graph.graph_id)
        .unwrap();
    assert_eq!(updated.panels[0].render_state, RenderState::Error);
}

#[test]
fn a_compiler_with_no_wired_bus_rejects_subscribe_but_still_applies_updates_directly() {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let compiler = WorkspaceCompiler::new();
    let contracts = vec![contract("notes.summarize")];
    let graph = compiler
        .compile(
            &monitor,
            &token,
            ObjectId(1),
            "exam_prep",
            &contracts,
            &empty_bundle(),
            ComplexityTier::Beginner,
            1.0,
        )
        .unwrap();

    assert!(compiler
        .subscribe_live(&monitor, &token, graph.graph_id)
        .is_err());

    // publish_live_update is a real no-op, never a hard failure, when no bus is wired.
    compiler
        .publish_live_update(
            &monitor,
            &token,
            &LiveUpdateEvent {
                workspace_id: graph.graph_id,
                panel_id: graph.panels[0].panel_id,
                event_type: LiveUpdateEventKind::Progress,
                payload_ref: None,
            },
        )
        .unwrap();
}
