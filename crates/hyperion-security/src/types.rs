use hyperion_knowledge_graph::NodeId;

pub type ActionId = u64;

/// docs/15 §7's four qualitatively distinct intervention levels — not
/// "the same dialog, different wording." Declared in ascending severity
/// so the derived `Ord` matches docs/15's `max(floor, level_for_score(...))`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InterventionLevel {
    SilentProceed,
    NotifyAndProceed,
    RequireExplicitConfirm,
    RequireBackupFirst,
}

/// Declared in ascending severity, matching [`InterventionLevel`]'s own convention, so a derived
/// `Ord` lets [`crate::engine::verify_action`] escalate via `.max(...)` rather than a manual match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SensitivityHint {
    Public,
    Personal,
    Sensitive,
    Restricted,
}

impl SensitivityHint {
    pub(crate) fn score(self) -> f32 {
        match self {
            SensitivityHint::Public => 0.0,
            SensitivityHint::Personal => 0.4,
            SensitivityHint::Sensitive => 0.75,
            SensitivityHint::Restricted => 1.0,
        }
    }
}

/// docs/17 §5/§6's `origin_type` on a Context object feeding an action —
/// the concept `is_provenance_tainted` walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginType {
    UserDirect,
    IngestedExternal,
    SystemDerived,
}

#[derive(Debug, Clone, Copy)]
pub struct ProvenanceNode {
    pub origin_type: OriginType,
    pub user_confirmed: bool,
}

/// docs/17's `IntentProvenanceChain`, narrowed to the one field the taint
/// floor (docs/15 §7) actually consumes.
#[derive(Debug, Clone)]
pub struct IntentProvenanceChain {
    pub action_id: ActionId,
    pub originating_intent_id: u64,
    pub derivation_path: Vec<ProvenanceNode>,
}

impl IntentProvenanceChain {
    /// docs/17 T1/T3: "flags if any context_object node has
    /// `origin_type == ingested-external` and `not user_confirmed`."
    pub fn is_tainted(&self) -> bool {
        self.derivation_path
            .iter()
            .any(|n| n.origin_type == OriginType::IngestedExternal && !n.user_confirmed)
    }
}

/// The inputs [`crate::engine::assess`] scores — this crate's narrowing of
/// docs/15 §7's `pending_action`, expressed as caller-supplied hints
/// rather than the full classifier pipeline (blast-radius/sensitivity/
/// reversibility classifiers) docs/15 assumes exist upstream. See this
/// crate's doc comment for what's deferred.
#[derive(Debug, Clone)]
pub struct PendingAction {
    pub action_id: ActionId,
    pub object_refs: Vec<NodeId>,
    /// docs/15 §7's `blast_radius_score` input — narrowed to "how many
    /// distinct objects does this action touch," saturating.
    pub scope_size: u32,
    /// Whether the action's effects can be reverted (e.g. via
    /// `hyperion-recovery`'s undo) — a boolean hint standing in for a
    /// real reversibility classifier.
    pub reversible: bool,
    pub sensitivity: SensitivityHint,
    /// docs/15 §7's `action.intent_engine_confidence`, `0.0..=1.0`.
    pub intent_confidence: f32,
    /// docs/15 §7's `score_corroboration(action)`, `0.0..=1.0` — docs/17
    /// T5 requires this be weighted *below* system-verified facts; this
    /// crate does not compute it (no Memory Engine integration here), it
    /// only accepts and weights it, which is exactly where T5's mitigation
    /// lives (see [`crate::engine::assess`]'s unconditional floor).
    pub corroboration: f32,
    pub provenance: Option<IntentProvenanceChain>,
}

/// docs/15 §4's `RiskAssessment` — the struct this crate's engine
/// produces.
#[derive(Debug, Clone)]
pub struct RiskAssessment {
    pub action_id: ActionId,
    pub blast_radius_score: f32,
    pub reversibility_score: f32,
    pub sensitivity_score: f32,
    pub confidence_score: f32,
    pub corroboration_score: f32,
    pub composite_score: f32,
    pub intervention_level: InterventionLevel,
    pub rationale: String,
    pub recovery_point_ref: Option<hyperion_recovery::RecoveryPointId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanaryResult {
    Pass,
    Fail {
        drift_millipoints: u32,
    },
    /// docs/17 T8: content hash mismatch — the artifact isn't what it
    /// claims to be, independent of any canary score.
    IntegrityMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromotionStatus {
    Blocked,
    Promoted,
}

/// docs/17 T8's `ModelIntegrityRecord`, narrowed to what
/// [`crate::model_integrity::canary_gate_model_promotion`] actually checks: a real Ed25519
/// signature check (docs/998-roadmap.md M9, via `hyperion-ai-runtime`'s own `verify`) for
/// "content-addressed + signature-verified," and a deterministic score-drift comparison standing
/// in for a real canary differential test suite.
#[derive(Debug, Clone, Copy)]
pub struct ModelIntegrityRecord {
    pub model_id: u64,
    pub signature_verified: bool,
    pub canary_result: CanaryResult,
    pub promotion_status: PromotionStatus,
}

#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("recovery error: {0}")]
    Recovery(#[from] hyperion_recovery::RecoveryError),
}
