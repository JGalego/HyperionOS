//! Mirrors every other crate in this workspace: every call is capability-
//! gated, re-checked live against the monitor.

use std::collections::HashMap;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::{Budget, ContextBundle, ExpertiseEstimate, ExpertiseLevel, Scope};
use hyperion_workspace::{
    CapabilityUiContract, ComplexityTier, RegionAffinity, WorkspaceCompiler, WorkspaceError,
};

fn contract() -> CapabilityUiContract {
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

#[test]
fn compile_requires_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();

    let compiler = WorkspaceCompiler::new();
    let contracts = vec![contract()];
    let result = compiler.compile(
        &monitor,
        &read_only,
        hyperion_storage::ObjectId(1),
        "goal",
        &contracts,
        &empty_bundle(),
        ComplexityTier::Beginner,
        1.0,
    );
    assert!(matches!(result, Err(WorkspaceError::Unauthorized)));
}

#[test]
fn revoking_a_token_blocks_further_access_re_checked_live() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let delegate = monitor
        .cap_derive(&root, RightsMask::all(), None, TrustBoundaryId(2))
        .unwrap();

    let compiler = WorkspaceCompiler::new();
    let contracts = vec![contract()];
    let graph = compiler
        .compile(
            &monitor,
            &delegate,
            hyperion_storage::ObjectId(1),
            "goal",
            &contracts,
            &empty_bundle(),
            ComplexityTier::Beginner,
            1.0,
        )
        .unwrap();
    assert!(compiler
        .get_graph(&monitor, &delegate, graph.graph_id)
        .is_ok());

    monitor.cap_revoke(&delegate);

    assert!(matches!(
        compiler.get_graph(&monitor, &delegate, graph.graph_id),
        Err(WorkspaceError::Unauthorized)
    ));
}
