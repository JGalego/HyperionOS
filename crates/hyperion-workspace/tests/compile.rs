//! docs/13-dynamic-ui-runtime.md's compile pipeline: capability-to-panel
//! mapping, Context Bundle binding, structural cache reuse, Adaptive
//! Complexity variants, and lifecycle transitions.

use std::collections::HashMap;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::{
    Budget, ContextBundle, ContextEntry, ExpertiseEstimate, ExpertiseLevel, InclusionMode, Scope,
};
use hyperion_workspace::{
    CapabilityUiContract, ComplexityTier, LifecycleState, PanelVariant, RegionAffinity,
    WorkspaceCompiler, WorkspaceError,
};

fn contract(capability_ref: &str, binds_category: Option<&str>) -> CapabilityUiContract {
    CapabilityUiContract {
        capability_ref: capability_ref.to_string(),
        panel_template: format!("{capability_ref}.default"),
        region_affinity: RegionAffinity::Center,
        min_size: (200, 200),
        priority: 0.5,
        binds_category: binds_category.map(|s| s.to_string()),
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
    (monitor, token, WorkspaceCompiler::new())
}

#[test]
fn compile_produces_one_panel_per_capability_with_a_derived_accessibility_node() {
    let (monitor, token, compiler) = setup();
    let contracts = vec![
        contract("notes.summarize", None),
        contract("calendar.read", None),
    ];
    let graph = compiler
        .compile(
            &monitor,
            &token,
            hyperion_storage::ObjectId(1),
            "exam_prep",
            &contracts,
            &empty_bundle(),
            ComplexityTier::Beginner,
            1.0,
        )
        .unwrap();

    assert_eq!(graph.panels.len(), 2);
    assert_eq!(graph.lifecycle_state, LifecycleState::Generating);
    for panel in &graph.panels {
        assert!(!panel.accessibility_node.accessible_name.is_empty());
    }
}

#[test]
fn a_second_compile_with_the_same_shape_is_a_cache_hit() {
    let (monitor, token, compiler) = setup();
    let contracts = vec![contract("notes.summarize", None)];
    compiler
        .compile(
            &monitor,
            &token,
            hyperion_storage::ObjectId(1),
            "exam_prep",
            &contracts,
            &empty_bundle(),
            ComplexityTier::Beginner,
            1.0,
        )
        .unwrap();
    compiler
        .compile(
            &monitor,
            &token,
            hyperion_storage::ObjectId(2),
            "exam_prep",
            &contracts,
            &empty_bundle(),
            ComplexityTier::Beginner,
            1.0,
        )
        .unwrap();

    let template = compiler
        .get_template("exam_prep", &contracts, ComplexityTier::Beginner)
        .unwrap();
    assert_eq!(
        template.hit_count, 2,
        "both compiles should have hit the same cached template"
    );
}

#[test]
fn a_different_complexity_tier_resolves_the_panel_variant() {
    let (monitor, token, compiler) = setup();
    let mut basic = contract("spreadsheet.formula_bar", None);
    basic.variants.insert(
        ComplexityTier::Pro,
        PanelVariant {
            panel_template: Some("spreadsheet.formula_bar.advanced".to_string()),
            min_size: Some((400, 100)),
        },
    );
    let contracts = vec![basic];

    let beginner = compiler
        .compile(
            &monitor,
            &token,
            hyperion_storage::ObjectId(1),
            "spreadsheet",
            &contracts,
            &empty_bundle(),
            ComplexityTier::Beginner,
            1.0,
        )
        .unwrap();
    let pro = compiler
        .compile(
            &monitor,
            &token,
            hyperion_storage::ObjectId(2),
            "spreadsheet",
            &contracts,
            &empty_bundle(),
            ComplexityTier::Pro,
            1.0,
        )
        .unwrap();

    assert_eq!(
        beginner.panels[0].min_size,
        (200, 200),
        "no Pro variant declared for beginner tier — base fields apply"
    );
    assert_eq!(
        pro.panels[0].min_size,
        (400, 100),
        "Pro variant's min_size override applies"
    );
}

#[test]
fn panels_bind_to_context_bundle_entries_matching_their_declared_category() {
    let (monitor, token, compiler) = setup();
    let contracts = vec![contract("notes.summarize", Some("notes"))];
    let mut bundle = empty_bundle();
    let note_id = hyperion_storage::ObjectId(42);
    bundle.entries.push(ContextEntry {
        category: "notes".to_string(),
        node_id: note_id,
        inclusion_mode: InclusionMode::Full,
        content: serde_json::json!({}),
        relevance_score: 1.0,
        source_signal: Vec::new(),
        generation: 0,
        captured_at: 0,
    });
    bundle.entries.push(ContextEntry {
        category: "calendar".to_string(),
        node_id: hyperion_storage::ObjectId(99),
        inclusion_mode: InclusionMode::Full,
        content: serde_json::json!({}),
        relevance_score: 1.0,
        source_signal: Vec::new(),
        generation: 0,
        captured_at: 0,
    });

    let graph = compiler
        .compile(
            &monitor,
            &token,
            hyperion_storage::ObjectId(1),
            "exam_prep",
            &contracts,
            &bundle,
            ComplexityTier::Beginner,
            1.0,
        )
        .unwrap();

    assert_eq!(graph.panels[0].bindings.len(), 1);
    assert_eq!(graph.panels[0].bindings[0].target, note_id);
}

#[test]
fn lifecycle_transitions_follow_docs_13_3_3() {
    let (monitor, token, compiler) = setup();
    let contracts = vec![contract("notes.summarize", None)];
    let graph = compiler
        .compile(
            &monitor,
            &token,
            hyperion_storage::ObjectId(1),
            "exam_prep",
            &contracts,
            &empty_bundle(),
            ComplexityTier::Beginner,
            1.0,
        )
        .unwrap();

    assert!(compiler.pin(&monitor, &token, graph.graph_id).is_ok()); // pin from Generating is allowed
    assert_eq!(
        compiler
            .get_graph(&monitor, &token, graph.graph_id)
            .unwrap()
            .lifecycle_state,
        LifecycleState::Pinned
    );

    let result = compiler.discard(&monitor, &token, graph.graph_id);
    assert!(
        matches!(result, Err(WorkspaceError::InvalidTransition(_))),
        "a pinned workspace must never be silently discarded"
    );
}

#[test]
fn mount_advances_generating_to_live() {
    let (monitor, token, compiler) = setup();
    let contracts = vec![contract("notes.summarize", None)];
    let graph = compiler
        .compile(
            &monitor,
            &token,
            hyperion_storage::ObjectId(1),
            "exam_prep",
            &contracts,
            &empty_bundle(),
            ComplexityTier::Beginner,
            1.0,
        )
        .unwrap();

    compiler.mount(&monitor, &token, graph.graph_id).unwrap();
    assert_eq!(
        compiler
            .get_graph(&monitor, &token, graph.graph_id)
            .unwrap()
            .lifecycle_state,
        LifecycleState::Live
    );

    let result = compiler.mount(&monitor, &token, graph.graph_id);
    assert!(
        matches!(result, Err(WorkspaceError::InvalidTransition(_))),
        "mounting twice must not be a no-op success"
    );
}
