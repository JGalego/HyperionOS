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

use crate::types::SemanticObjectRef;

const DISAMBIGUATION_CATEGORY: &str = "web_disambiguation_candidate";

/// docs/19 §10's human-in-the-loop disambiguation, made real: wraps a
/// [`SemanticObjectRef`] (typically one [`crate::NetstackHub::web_research`]
/// flagged `needs_review: true`, though this crate leaves that check to
/// the caller — the same "caller decides when to invoke" precedent
/// `hyperion-semantic-fs`/`hyperion-compat`'s own `present_as_workspace`
/// already established) in a synthetic Context Bundle and compiles it
/// through the real `hyperion-workspace` Phase 5 pipeline, closing this
/// crate's own "surfacing `needs_review` through an active Workspace is
/// [13]'s concern and not wired into this crate" gap.
pub fn present_disambiguation_as_workspace(
    compiler: &WorkspaceCompiler,
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    resolved: &SemanticObjectRef,
    intent_id: NodeId,
    now: u64,
) -> Result<WorkspaceGraph, WorkspaceError> {
    let bundle = ContextBundle {
        bundle_id: resolved.object_id.0,
        scope: Scope {
            intent_id: "web_disambiguation".to_string(),
            session_id: "hyperion-netstack".to_string(),
            mentions: Vec::new(),
            anchors: Vec::new(),
        },
        entries: vec![ContextEntry {
            category: DISAMBIGUATION_CATEGORY.to_string(),
            node_id: resolved.object_id,
            inclusion_mode: InclusionMode::Full,
            content: serde_json::Value::Null,
            relevance_score: 1.0,
            source_signal: vec!["hyperion-netstack".to_string()],
            generation: 0,
            captured_at: now,
        }],
        assembled_at: now,
        budget: Budget::default(),
        expertise_signal: ExpertiseEstimate {
            domain: "web_research".to_string(),
            level: ExpertiseLevel::Novice,
            evidence: Vec::new(),
            confidence: 0.0,
        },
    };

    let contract = CapabilityUiContract {
        capability_ref: "netstack.disambiguation_review".to_string(),
        panel_template: "netstack.disambiguation_review.default".to_string(),
        region_affinity: RegionAffinity::Center,
        min_size: (400, 300),
        priority: 1.0,
        binds_category: Some(DISAMBIGUATION_CATEGORY.to_string()),
        variants: HashMap::new(),
        accessible_role: Some("dialog".to_string()),
        label_template: Some("Confirm this is the entity you meant".to_string()),
        keyboard_operations: vec!["confirm".to_string(), "reject".to_string()],
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
        "web_disambiguation",
        &[contract],
        &bundle,
        ComplexityTier::Beginner,
        1.0,
    )
}
