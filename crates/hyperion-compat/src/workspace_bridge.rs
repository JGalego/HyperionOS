use std::collections::HashMap;

use hyperion_capability::{CapabilityMonitor, CapabilityToken};
use hyperion_context::{
    Budget, ContextBundle, ContextEntry, ExpertiseEstimate, ExpertiseLevel, InclusionMode, Scope,
};
use hyperion_knowledge_graph::NodeId;
use hyperion_workspace::{
    CapabilityUiContract, ComplexityTier, RegionAffinity, WorkspaceCompiler, WorkspaceGraph,
};

use crate::host::CompatHost;
use crate::types::{AccessibilityBridgeTier, CompatError, SessionId};

const COMPAT_ARTIFACT_CATEGORY: &str = "compat_artifact";

/// docs/27's "Window-to-Workspace binding": wraps a running Compatibility
/// session as the sole content of an otherwise ordinary Workspace,
/// compiled through the real `hyperion-workspace` Phase 5 pipeline —
/// closes this crate's own "there is no legacy-application Workspace
/// type here to carry [the bounded accessibility exception]" gap, and
/// `hyperion-workspace`'s matching gap on its own side. The session's
/// promoted artifacts (Stage B `IngestedArtifact`s, never Stage A
/// captures) are bound to the panel exactly like any natively generated
/// panel binds to its Context Bundle entries — "to the rest of Hyperion
/// this Workspace is indistinguishable in kind from a natively generated
/// one," per docs/27, even though its *content* is opaque rather than a
/// declaratively composed Capability.
///
/// The accessibility node implements docs/27's "Accessibility bridging
/// (bounded exception to Invariant 6)" literally: its `accessible_name`
/// IS the doc's disclosure text, "Limited accessibility: legacy
/// application," whenever `session.profile.accessibility_bridge` is not
/// `Platform` — never a normal-looking node for opaque pixel content this
/// crate has no real Capability contract to derive one from honestly.
/// This crate still runs no real platform accessibility bridge or pixel-
/// level OCR fallback (see this crate's doc comment) — what's real here
/// is the bounded-exception bookkeeping and disclosure docs/27 itself
/// requires regardless of which tier produced the underlying tree.
pub fn present_as_workspace(
    host: &CompatHost,
    compiler: &WorkspaceCompiler,
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    session_id: SessionId,
    intent_id: NodeId,
    now: u64,
) -> Result<WorkspaceGraph, CompatError> {
    let session = host.session(session_id).ok_or(CompatError::NoSuchSession)?;

    let entries = host
        .promoted_artifacts(session_id)
        .into_iter()
        .filter_map(|artifact| artifact.promoted_object_id)
        .map(|node_id| ContextEntry {
            category: COMPAT_ARTIFACT_CATEGORY.to_string(),
            node_id,
            inclusion_mode: InclusionMode::Reference,
            content: serde_json::Value::Null,
            relevance_score: 1.0,
            source_signal: vec!["hyperion-compat".to_string()],
            generation: 0,
            captured_at: now,
        })
        .collect();

    let bundle = ContextBundle {
        bundle_id: session_id,
        scope: Scope {
            intent_id: "legacy_app_session".to_string(),
            session_id: session_id.to_string(),
            mentions: Vec::new(),
            anchors: Vec::new(),
        },
        entries,
        assembled_at: now,
        budget: Budget::default(),
        expertise_signal: ExpertiseEstimate {
            domain: "compatibility".to_string(),
            level: ExpertiseLevel::Novice,
            evidence: Vec::new(),
            confidence: 0.0,
        },
    };

    let accessible_name = match session.profile.accessibility_bridge {
        AccessibilityBridgeTier::Platform => format!("{:?} application", session.profile.target),
        AccessibilityBridgeTier::PixelFallback | AccessibilityBridgeTier::None => {
            "Limited accessibility: legacy application".to_string()
        }
    };

    let contract = CapabilityUiContract {
        capability_ref: "compat.legacy_session".to_string(),
        panel_template: "compat.legacy_session.default".to_string(),
        region_affinity: RegionAffinity::Center,
        min_size: (640, 480),
        priority: 1.0,
        binds_category: Some(COMPAT_ARTIFACT_CATEGORY.to_string()),
        variants: HashMap::new(),
        accessible_role: Some("application".to_string()),
        label_template: Some(accessible_name),
        keyboard_operations: Vec::new(),
        alt_text_hook: None,
        contrast_ratio: 7.0,
        has_motion: false,
        reduced_motion_alternative: true,
        language_tag: "en".to_string(),
        emits_audio: false,
        has_visual_alert_equivalent: true,
    };

    Ok(compiler.compile(
        monitor,
        token,
        intent_id,
        "legacy_app_session",
        &[contract],
        &bundle,
        ComplexityTier::Beginner,
        1.0,
    )?)
}
