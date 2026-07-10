//! docs/14-accessibility.md §5.1/§5.2/§10: the linter rule set, the
//! never-nameless-node fallback derivation, and the "lint failure ->
//! fallback template" guarantee.

use std::collections::HashMap;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::{Budget, ContextBundle, ExpertiseEstimate, ExpertiseLevel, Scope};
use hyperion_workspace::{CapabilityUiContract, ComplexityTier, RegionAffinity, WorkspaceCompiler};

fn base_contract() -> CapabilityUiContract {
    CapabilityUiContract {
        capability_ref: "notes.summarize".to_string(),
        panel_template: "notes.default".to_string(),
        region_affinity: RegionAffinity::Left,
        min_size: (200, 200),
        priority: 0.5,
        binds_category: None,
        variants: HashMap::new(),
        accessible_role: Some("region".to_string()),
        label_template: Some("Notes".to_string()),
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
fn a_contract_with_no_accessibility_metadata_still_gets_a_valid_generic_node() {
    let (monitor, token, compiler) = setup();
    let mut contract = base_contract();
    contract.accessible_role = None;
    contract.label_template = None;
    let contracts = vec![contract];

    let graph = compiler
        .compile(
            &monitor,
            &token,
            hyperion_storage::ObjectId(1),
            "goal",
            &contracts,
            &empty_bundle(),
            ComplexityTier::Beginner,
            1.0,
        )
        .unwrap();

    let node = &graph.panels[0].accessibility_node;
    assert!(
        !node.accessible_name.is_empty(),
        "nothing is ever silently inaccessible"
    );
    assert_eq!(node.role, "generic");
    assert_eq!(
        node.accessible_name, "notes summarize",
        "derived from the capability_ref itself"
    );
}

#[test]
fn low_contrast_fails_the_linter_and_the_template_falls_back_to_the_generic_viewer() {
    let (monitor, token, compiler) = setup();
    let mut contract = base_contract();
    contract.contrast_ratio = 2.0; // below the 4.5:1 minimum
    let contracts = vec![contract];

    let graph = compiler
        .compile(
            &monitor,
            &token,
            hyperion_storage::ObjectId(1),
            "goal",
            &contracts,
            &empty_bundle(),
            ComplexityTier::Beginner,
            1.0,
        )
        .unwrap();

    assert_eq!(graph.panels.len(), 1);
    assert_eq!(
        graph.panels[0].capability_ref, "generic.raw_data_viewer",
        "a failing template must fall back, never render the bad one"
    );

    let template = compiler
        .get_template("goal", &contracts, ComplexityTier::Beginner)
        .unwrap();
    assert!(
        template.lint_result.passed,
        "the cached fallback itself must be lint-clean"
    );
}

#[test]
fn undersized_target_fails_the_linter() {
    let (monitor, token, compiler) = setup();
    let mut contract = base_contract();
    contract.min_size = (10, 10); // well under 44dp
    let contracts = vec![contract];

    compiler
        .compile(
            &monitor,
            &token,
            hyperion_storage::ObjectId(1),
            "goal",
            &contracts,
            &empty_bundle(),
            ComplexityTier::Beginner,
            1.0,
        )
        .unwrap();

    let template = compiler
        .get_template("goal", &contracts, ComplexityTier::Beginner)
        .unwrap();
    assert!(
        template.lint_result.passed,
        "must have been replaced by the fallback template"
    );
    assert_eq!(template.panels.len(), 1);
    assert_eq!(template.panels[0].capability_ref, "generic.raw_data_viewer");
}

#[test]
fn an_audio_only_alert_with_no_visual_equivalent_fails_the_linter() {
    let (monitor, token, compiler) = setup();
    let mut contract = base_contract();
    contract.emits_audio = true;
    contract.has_visual_alert_equivalent = false;
    let contracts = vec![contract];

    compiler
        .compile(
            &monitor,
            &token,
            hyperion_storage::ObjectId(1),
            "goal",
            &contracts,
            &empty_bundle(),
            ComplexityTier::Beginner,
            1.0,
        )
        .unwrap();

    let template = compiler
        .get_template("goal", &contracts, ComplexityTier::Beginner)
        .unwrap();
    assert_eq!(
        template.panels[0].capability_ref, "generic.raw_data_viewer",
        "audio-only alert must be rejected exactly like a nameless node"
    );
}

#[test]
fn a_well_formed_contract_passes_the_linter_without_any_fallback() {
    let (monitor, token, compiler) = setup();
    let contracts = vec![base_contract()];

    let graph = compiler
        .compile(
            &monitor,
            &token,
            hyperion_storage::ObjectId(1),
            "goal",
            &contracts,
            &empty_bundle(),
            ComplexityTier::Beginner,
            1.0,
        )
        .unwrap();

    assert_eq!(graph.panels[0].capability_ref, "notes.summarize");
}
