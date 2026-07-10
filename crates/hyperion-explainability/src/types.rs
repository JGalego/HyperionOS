use hyperion_knowledge_graph::NodeId;
use hyperion_privacy::SensitivityClass;
use hyperion_recovery::RecoveryPointId;

pub type ExplanationId = u64;
pub type ActionId = u64;

/// docs/18 §4's `ControlState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlState {
    Proposed,
    Executing,
    Completed,
    Interrupted,
    Modified,
    RolledBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfidenceMethod {
    SelfConsistency,
    Verifier,
    Ensemble,
    Heuristic,
}

/// docs/18 §4's `ConfidenceScore`. `calibration_set_ref` is omitted — no
/// crate in this workspace has a calibration-set concept yet.
#[derive(Debug, Clone, Copy)]
pub struct ConfidenceScore {
    pub value: f32,
    pub method: ConfidenceMethod,
}

/// docs/18 §4's `EvidenceRef`.
#[derive(Debug, Clone)]
pub struct EvidenceRef {
    pub object_id: NodeId,
    pub excerpt_or_summary: String,
    pub weight: f32,
}

/// docs/18 §4's `ReasoningStep`, `tool_or_capability_used` narrowed to a
/// plain capability-ref string (this workspace has no single
/// `CapabilityId` newtype threaded everywhere).
#[derive(Debug, Clone)]
pub struct ReasoningStep {
    pub step_index: u32,
    pub description: String,
    pub capability_ref: Option<String>,
    pub inputs_ref: Vec<NodeId>,
    pub output_ref: Option<NodeId>,
}

/// docs/18 §4's `Alternative`.
#[derive(Debug, Clone)]
pub struct Alternative {
    pub description: String,
    pub score: f32,
    pub rejection_reason: String,
}

/// docs/18 §4's `ExplanationRecord` — CLAUDE.md's "why/how/evidence/
/// confidence/undo" framing, plus the doc's two extra security/privacy
/// tie-in fields (`trust_boundary_span`, `privacy_class`) and the DAG-
/// composition fields (`parent_records`/`child_records`) multi-agent
/// merge needs. "How" is `reasoning_chain`+`evidence`; "why" is
/// `reasoning_chain`+`alternatives`.
#[derive(Debug, Clone)]
pub struct ExplanationRecord {
    pub id: ExplanationId,
    pub action_id: ActionId,
    pub triggering_intent_id: u64,
    pub agent_id: u64,
    pub capability_ref: String,
    pub created_at: u64,
    pub reasoning_chain: Vec<ReasoningStep>,
    pub evidence: Vec<EvidenceRef>,
    pub confidence: Option<ConfidenceScore>,
    pub alternatives: Vec<Alternative>,
    pub undo_ref: Option<RecoveryPointId>,
    pub trust_boundary_span: Vec<u64>,
    pub privacy_class: Option<SensitivityClass>,
    pub parent_records: Vec<ExplanationId>,
    pub child_records: Vec<ExplanationId>,
    pub control_state: ControlState,
}

/// docs/18 §6's `depth` parameter to `explain.query`/`resolve_why`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    Headline,
    Full,
}

/// docs/18 §5's `render_at_complexity_level` result, narrowed to a
/// deterministic template-rendered headline (see this crate's doc
/// comment on deferred real NLG) plus an optional full record and its
/// resolved parent chain at `Depth::Full`.
#[derive(Debug, Clone)]
pub struct ExplanationView {
    pub headline: String,
    pub full: Option<ExplanationRecord>,
    pub parents: Vec<ExplanationView>,
}

#[derive(Debug, thiserror::Error)]
pub enum ExplainabilityError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("no such explanation record")]
    NoSuchRecord,
}
