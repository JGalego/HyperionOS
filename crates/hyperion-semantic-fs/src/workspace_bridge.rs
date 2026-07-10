use std::collections::HashMap;

use hyperion_capability::{CapabilityMonitor, CapabilityToken};
use hyperion_context::{
    Budget, ContextBundle, ContextEntry, ExpertiseEstimate, ExpertiseLevel, InclusionMode, Scope,
};
use hyperion_knowledge_graph::NodeId;
use hyperion_workspace::{
    CapabilityUiContract, ComplexityTier, RegionAffinity, WorkspaceCompiler, WorkspaceError,
    WorkspaceGraph,
};

use crate::types::VirtualFolder;

const SEARCH_RESULT_CATEGORY: &str = "search_result";

/// This phase's "workspace generation" mandate item: wraps a
/// [`VirtualFolder`]'s members in a synthetic Context Bundle and compiles
/// it through the real `hyperion-workspace` Phase 5 compiler, so search
/// results have somewhere to be displayed without this crate reimplementing
/// any UI logic of its own.
pub fn present_as_workspace(
    compiler: &WorkspaceCompiler,
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    folder: &VirtualFolder,
    intent_id: NodeId,
) -> Result<WorkspaceGraph, WorkspaceError> {
    let entries = folder
        .member_object_ids
        .iter()
        .map(|&id| ContextEntry {
            category: SEARCH_RESULT_CATEGORY.to_string(),
            node_id: id,
            inclusion_mode: InclusionMode::Reference,
            content: serde_json::Value::Null,
            relevance_score: 1.0,
            source_signal: vec!["semantic_filesystem".to_string()],
            generation: 0,
            captured_at: folder.materialized_at,
        })
        .collect();

    let bundle = ContextBundle {
        bundle_id: folder.folder_id,
        scope: Scope {
            intent_id: "universal_search".to_string(),
            session_id: "semantic_filesystem".to_string(),
            mentions: Vec::new(),
            anchors: Vec::new(),
        },
        entries,
        assembled_at: folder.materialized_at,
        budget: Budget::default(),
        expertise_signal: ExpertiseEstimate {
            domain: "search".to_string(),
            level: ExpertiseLevel::Novice,
            evidence: Vec::new(),
            confidence: 0.0,
        },
    };

    let contract = CapabilityUiContract {
        capability_ref: "fs.search_results".to_string(),
        panel_template: "fs.search_results.default".to_string(),
        region_affinity: RegionAffinity::Center,
        min_size: (400, 400),
        priority: 1.0,
        binds_category: Some(SEARCH_RESULT_CATEGORY.to_string()),
        variants: HashMap::new(),
        accessible_role: Some("list".to_string()),
        label_template: Some("Search results".to_string()),
        keyboard_operations: vec!["open".to_string()],
        alt_text_hook: None,
        contrast_ratio: 7.0,
        has_motion: false,
        reduced_motion_alternative: true,
        language_tag: "en".to_string(),
        emits_audio: false,
        has_visual_alert_equivalent: true,
    };

    compiler.compile(
        monitor,
        token,
        intent_id,
        "universal_search",
        &[contract],
        &bundle,
        ComplexityTier::Beginner,
        1.0,
    )
}
