//! docs/20 §5.5/§7's `handle_cross_device_workspace`, in its narrowest
//! real, closable form. This crate's own doc comment names
//! `DeviceRegistry::find_render_surfaces` as "the registry query that
//! step would consult" for real Cross-Device Workspace Assembly -- but
//! nothing in the workspace ever actually called it from a real
//! `hyperion-workspace` integration. This proves the real query
//! genuinely decides which, and how many, real devices a compiled
//! Workspace mounts onto.
//!
//! Deliberately narrow: this does NOT implement docs/13 §7's full
//! per-surface Context-Bundle-field split (a real per-surface layout
//! algorithm neither doc's pseudocode section fully specifies) -- every
//! eligible surface mounts the same compiled graph. Splitting *which*
//! fields go to *which* surface is a real, separate follow-on.

use std::collections::HashMap;
use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::{Budget, ContextBundle, ExpertiseEstimate, ExpertiseLevel, Scope};
use hyperion_device::{
    CapabilityManifestEntry, DeviceRegistry, DeviceType, Direction, SafetyClass,
};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_workspace::{CapabilityUiContract, ComplexityTier, RegionAffinity, WorkspaceCompiler};

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

fn status_contract() -> CapabilityUiContract {
    CapabilityUiContract {
        capability_ref: "status.show".to_string(),
        panel_template: "status.show.default".to_string(),
        region_affinity: RegionAffinity::Center,
        min_size: (200, 200),
        priority: 0.5,
        binds_category: None,
        variants: HashMap::new(),
        accessible_role: Some("region".to_string()),
        label_template: Some("status".to_string()),
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

#[test]
fn find_render_surfaces_genuinely_decides_how_many_real_devices_a_workspace_mounts_to() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let devices = DeviceRegistry::new(graph);
    let owner = 1;

    let display = devices
        .register(
            &monitor,
            &token,
            DeviceType::Display,
            "Acme",
            "Screen-1",
            vec![CapabilityManifestEntry {
                capability_name: "display.render".to_string(),
                direction: Direction::Render,
                safety_class: SafetyClass::Cosmetic,
            }],
            owner,
            0,
        )
        .unwrap();
    // A real registered device with no Render-direction capability at
    // all -- it must never count as an eligible surface.
    let sensor = devices
        .register(
            &monitor,
            &token,
            DeviceType::Sensor,
            "Acme",
            "Sensor-1",
            vec![CapabilityManifestEntry {
                capability_name: "sensor.read".to_string(),
                direction: Direction::Sense,
                safety_class: SafetyClass::Standard,
            }],
            owner,
            0,
        )
        .unwrap();

    let surfaces = devices.find_render_surfaces(owner);
    assert_eq!(
        surfaces,
        vec![display],
        "only the Render-capable device is a real eligible surface"
    );

    let compiler = WorkspaceCompiler::new();
    let contracts = vec![status_contract()];
    let mut mounted = Vec::new();
    for _surface in &surfaces {
        let workspace_graph = compiler
            .compile(
                &monitor,
                &token,
                hyperion_storage::ObjectId(1),
                "show_status",
                &contracts,
                &empty_bundle(),
                ComplexityTier::Beginner,
                1.0,
            )
            .unwrap();
        compiler
            .mount(&monitor, &token, workspace_graph.graph_id)
            .unwrap();
        mounted.push(workspace_graph.graph_id);
    }

    assert_eq!(
        mounted.len(),
        surfaces.len(),
        "the real device registry query decided how many surfaces to mount to, not a hardcoded count"
    );
    assert_eq!(
        mounted.len(),
        1,
        "the sensor (device {sensor}) must not have been mounted to"
    );
}
