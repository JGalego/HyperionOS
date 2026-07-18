use hyperion_knowledge_graph::NodeId;

pub type ExplanationId = u64;
pub type ActionId = u64;

/// A narrowed local copy of `hyperion-recovery`'s own `RecoveryPointId` (itself a bare `u64`
/// alias) -- this crate's own previously-named "agent-runtime/explainability Cargo cycle" gap,
/// closed by never depending on `hyperion-recovery` (or, transitively through it,
/// `hyperion-privacy`) just for a plain integer id. `hyperion_recovery::RecoveryPointId` values
/// pass through here unchanged (`type` aliases are structurally transparent), so no caller-side
/// conversion is needed.
pub type RecoveryPointId = u64;

/// A narrowed local copy of `hyperion-privacy`'s own `SensitivityClass` -- the same
/// dependency-avoidance this crate's `RecoveryPointId` above uses, and the same precedent
/// `hyperion-security::SensitivityHint` already established for the identical reason. Every
/// caller in this workspace always passes `None` for the field this backs today (see
/// [`ExplanationRecord::privacy_class`]'s own doc comment) — no real value ever needs to convert
/// between the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SensitivityClass {
    Public,
    Personal,
    Sensitive,
    Restricted,
}

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
    /// Recorded for a future consumer to act on, never enforced by this crate -- see this
    /// crate's own doc comment's "Encryption-at-privacy-tier of the store itself" bullet. Every
    /// real caller in this workspace passes `None` today; no crate here has a real signal to
    /// populate it with yet.
    pub privacy_class: Option<SensitivityClass>,
    pub parent_records: Vec<ExplanationId>,
    pub child_records: Vec<ExplanationId>,
    pub control_state: ControlState,
}

/// docs/18 §9's own degrade-path contract: `explain.query` must never present a best-effort
/// reconstruction as if it were the real, causally-recorded record. `Authoritative` came from
/// [`crate::ExplanationStore::get`] itself; `Reconstructed` was rebuilt from
/// [31 — Event System](../31-event-system.md) logs by
/// [`crate::ExplanationStore::get_or_reconstruct`] because the real record was genuinely absent —
/// see that function's own doc comment for exactly which fields a reconstruction can and can't
/// recover.
#[derive(Debug, Clone)]
pub enum ExplanationLookup {
    Authoritative(ExplanationRecord),
    Reconstructed(ExplanationRecord),
}

impl ExplanationLookup {
    pub fn record(&self) -> &ExplanationRecord {
        match self {
            ExplanationLookup::Authoritative(r) | ExplanationLookup::Reconstructed(r) => r,
        }
    }

    pub fn is_reconstructed(&self) -> bool {
        matches!(self, ExplanationLookup::Reconstructed(_))
    }
}

/// docs/18 §6's `depth` parameter to `explain.query`/`resolve_why`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    Headline,
    Full,
}

/// A [`ReasoningStep`] with its `inputs_ref`/`output_ref` resolved into human-readable text via
/// `hyperion_knowledge_graph::NodeRecord::display_label` — [`crate::render::resolve_why_with_graph`]'s
/// own real answer to this crate's previously-named "NodeIds never resolved via the KG" gap:
/// [`ReasoningStep`] itself keeps storing bare `NodeId`s (the record is still the durable, replayable
/// source of truth), but a renderer building this view for a person gets real descriptions instead.
#[derive(Debug, Clone)]
pub struct ResolvedReasoningStep {
    pub step_index: u32,
    pub description: String,
    /// One resolved description per `ReasoningStep::inputs_ref` entry, same order. A reference
    /// that's no longer visible to the caller (tombstoned, or genuinely gone) resolves to an
    /// honest placeholder rather than an error — a stale historical reference is expected drift,
    /// not a bug worth failing the whole explanation over.
    pub inputs: Vec<String>,
    pub output: Option<String>,
}

/// An [`EvidenceRef`] with its `object_id` resolved the same way [`ResolvedReasoningStep`]
/// resolves a step's own references.
#[derive(Debug, Clone)]
pub struct ResolvedEvidence {
    pub label: String,
    pub excerpt_or_summary: String,
    pub weight: f32,
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
    /// docs/18 §9: "never presenting a best-effort guess as an authoritative record." `true`
    /// when [`crate::ExplanationStore::get_or_reconstruct`] had to rebuild this view from
    /// [31 — Event System](../31-event-system.md) logs rather than the real, causally-recorded
    /// store — a renderer must surface this, not silently drop it.
    pub reconstructed: bool,
    /// Populated only by [`crate::render::resolve_why_with_graph`] (empty from plain
    /// [`crate::render::resolve_why`], which has no `KnowledgeGraph` handle to resolve against) —
    /// see [`ResolvedReasoningStep`].
    pub resolved_reasoning_chain: Vec<ResolvedReasoningStep>,
    /// Populated only by [`crate::render::resolve_why_with_graph`] — see [`ResolvedEvidence`].
    pub resolved_evidence: Vec<ResolvedEvidence>,
}

/// docs/18 §10/§13's "rolling Brier score per Agent/Capability... feeding an alert if an Agent's
/// stated confidence systematically diverges from observed outcomes" — a real, computed
/// calibration summary for one `(agent_id, capability_ref)` pair, over every terminal
/// (`ControlState::Completed`/`ControlState::RolledBack` — matching [`crate::ExplanationStore::
/// incomplete`]'s own convention for what counts as resolved) record this crate holds a real
/// `confidence` for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalibrationScore {
    /// The rolling Brier score itself: the mean squared error between each record's own
    /// `confidence.value` (the stated probability of success) and its real observed outcome
    /// (`1.0` for `Completed`, `0.0` for `RolledBack`) — `0.0` is perfect calibration, `1.0` is
    /// the worst possible score.
    pub brier_score: f32,
    /// How many real, terminal, confidence-scored records this score is computed over — a score
    /// over few samples isn't yet a reliable signal, named explicitly rather than silently
    /// treated the same as a well-sampled one.
    pub sample_count: usize,
    /// docs/18 §13's own alert condition: `true` once `brier_score` crosses a real threshold with
    /// enough samples to trust the signal — see [`crate::calibration`]'s own doc comment for the
    /// exact numbers and their reasoning.
    pub alert: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ExplainabilityError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("no such explanation record")]
    NoSuchRecord,
}
