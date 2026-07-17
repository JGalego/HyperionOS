use hyperion_explainability::ExplanationId;
use hyperion_intent::HandleOutcome;
use hyperion_knowledge_graph::NodeId;
use hyperion_plugin_framework::PluginId;
use hyperion_security::{IntentProvenanceChain, InterventionLevel, SensitivityHint};

/// docs/26 §2's per-endpoint scope strings (`"intent:submit"`,
/// `"memory:erase"`, ...), as a closed enum instead of open strings —
/// this crate's caller can't typo a scope name past the compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ApiScope {
    IntentSubmit,
    MemoryWrite,
    /// Gates [`crate::gateway::ApiGateway::check_skill_delegation_signal`] — the one read this
    /// crate does over Procedural memory today. No generic `memory.query` endpoint exists yet
    /// (unlike `KgQuery`'s Knowledge Graph counterpart); this scope is scoped to that one real
    /// read, not a general memory-query capability.
    MemoryQuery,
    KgQuery,
    KgWrite,
    CapabilityInvoke,
    ContextAssemble,
}

/// docs/26's own named "Rate/quota enforcement... no algorithm given" gap: a real, per-caller
/// fixed-window counter policy for [`crate::gateway::ApiGateway::invoke_capability`], the same
/// algorithm `hyperion-netstack`'s own `DomainEgressGrant` rate limiting already established.
/// `Default` is a generous, always-applied floor every caller gets until
/// [`crate::gateway::ApiGateway::set_rate_limit`] overrides it for one specific token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitPolicy {
    pub calls_per_window: u32,
    pub window_secs: u64,
}

impl Default for RateLimitPolicy {
    fn default() -> Self {
        RateLimitPolicy {
            calls_per_window: 60,
            window_secs: 60,
        }
    }
}

/// docs/26 §2's Intent API `SubmitIntentRequest`, narrowed to what
/// `hyperion-intent::IntentEngine::handle_utterance` actually takes.
#[derive(Debug, Clone)]
pub struct SubmitIntentRequest {
    pub utterance: String,
    pub session_id: String,
}

/// docs/26 §2's `SubmitIntentResponse`, reshaped as an enum around
/// `hyperion_intent::HandleOutcome` rather than a flat struct with an
/// always-present `intentId` — `NeedsClarification` has no intent id to
/// give one.
#[derive(Debug, Clone)]
pub enum SubmitIntentResponse {
    Submitted {
        intent_id: NodeId,
    },
    NeedsClarification {
        mention: String,
        candidates: Vec<NodeId>,
    },
    /// docs/998-roadmap.md's Backlog "Protect the Human" item, surfaced through the gateway: the
    /// caller's session is in `hyperion-intent`'s real think mode, so `intent_id` names a real,
    /// paused root Intent — call `hyperion_intent::IntentEngine::proceed_with_decomposition`
    /// directly (this gateway has no route for it yet; see this crate's doc comment) once the
    /// human's own reasoning has run.
    PendingThink {
        intent_id: NodeId,
    },
}

impl From<HandleOutcome> for SubmitIntentResponse {
    fn from(outcome: HandleOutcome) -> Self {
        match outcome {
            HandleOutcome::Submitted(id) => SubmitIntentResponse::Submitted { intent_id: id },
            HandleOutcome::PendingThink(id) => SubmitIntentResponse::PendingThink { intent_id: id },
            HandleOutcome::NeedsClarification {
                mention,
                candidates,
            } => SubmitIntentResponse::NeedsClarification {
                mention,
                candidates,
            },
        }
    }
}

/// Caller-supplied risk-classifier hints, threaded straight into
/// `hyperion_security::PendingAction` — that crate's own doc comment
/// frames `scope_size`/`reversible`/`SensitivityHint` etc. as "caller-
/// supplied hints rather than the full classifier pipeline docs/15 §7
/// assumes exist upstream," so the gateway does not attempt to derive
/// them itself; it just forwards whatever the caller asserts.
///
/// [`Default`] reads as the least-risky action (a single, reversible,
/// public, fully-confident, fully-corroborated touch) — the same
/// silent-proceed outcome every `invoke_capability` call had before this
/// gate existed, so call sites that aren't exercising risk assessment
/// don't need to construct one field-by-field.
#[derive(Debug, Clone)]
pub struct RiskHints {
    pub object_refs: Vec<NodeId>,
    pub scope_size: u32,
    pub reversible: bool,
    pub sensitivity: SensitivityHint,
    pub intent_confidence: f32,
    pub corroboration: f32,
    pub provenance: Option<IntentProvenanceChain>,
}

impl Default for RiskHints {
    fn default() -> Self {
        RiskHints {
            object_refs: Vec::new(),
            scope_size: 1,
            reversible: true,
            sensitivity: SensitivityHint::Public,
            intent_confidence: 1.0,
            corroboration: 1.0,
            provenance: None,
        }
    }
}

/// docs/26 §2's Capability Invocation API `InvokeRequest` — never names
/// an Implementation, only a `contractId`; selection is delegated
/// entirely to the registry + the real `hyperion-model-router` (see
/// this crate's doc comment). `risk`/`confirmed` feed docs/15's real
/// Risk-Assessment Engine gate: an action classified
/// `RequireExplicitConfirm` or higher is rejected unless `confirmed` is
/// set, and `RequireBackupFirst` additionally requires (and gets) a real
/// `hyperion-recovery` recovery point before it's allowed to proceed.
#[derive(Debug, Clone)]
pub struct InvokeRequest {
    pub contract_id: String,
    pub inputs: serde_json::Value,
    pub agent_id: u64,
    pub intent_id: u64,
    pub risk: RiskHints,
    pub confirmed: bool,
}

/// docs/26 §2's `InvokeResponse`. `ensemble` is `Some` exactly when
/// [`crate::gateway::ApiGateway::invoke_capability`] actually dispatched a real second,
/// architecturally distinct implementation to verify against the primary — see
/// [`EnsembleOutcome`] and this crate's own doc comment.
#[derive(Debug, Clone)]
pub struct InvokeResponse {
    pub outputs: serde_json::Value,
    pub implementation_used: PluginId,
    pub explanation_id: ExplanationId,
    pub ensemble: Option<EnsembleOutcome>,
}

/// docs/23 §Pseudocode's `reconcile_ensemble`, `Agreed` case: a real second, architecturally
/// distinct implementation (different `hyperion_model_router::ImplKind` from the primary) was
/// actually dispatched — never merely estimated — and produced the identical real output, so the
/// primary's confidence is genuinely boosted (see [`crate::router_bridge::boost_confidence`]),
/// not just asserted. There is no `Disagreed` variant here: this crate has no
/// `designated_tiebreaker` to consult (docs/23's `SemanticContract` equivalent carries none), so
/// an unresolvable disagreement is never returned as a success value — see
/// [`ApiError::EnsembleDisagreement`] instead, matching docs/23's own routing of that case to a
/// human, not to a silently-chosen winner.
#[derive(Debug, Clone)]
pub struct EnsembleOutcome {
    pub verifying_impl: PluginId,
    pub boosted_confidence: f32,
}

/// docs/998-roadmap.md's Backlog "Protect the Human" item: "no signal exists for 'you've
/// delegated this kind of task N times this month, want to do the next one yourself?'" — returned
/// by [`crate::gateway::ApiGateway::check_skill_delegation_signal`] only once the real count
/// crosses the caller-supplied threshold; `None` below it. Deliberately advisory, never enforced —
/// CLAUDE.md's own User Control principle ("Hyperion assists. It does not control.") means this
/// never blocks or refuses `entity_key`'s capability, only records a real, explainable reason a
/// caller *might* want to ask the user that question.
#[derive(Debug, Clone)]
pub struct SkillDelegationSignal {
    pub entity_key: String,
    pub count: usize,
    pub threshold: usize,
    pub explanation_id: ExplanationId,
}

/// docs/26 §5: no canonical error envelope is given in the doc — "an
/// action item for your Rust code." This is that envelope.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("capability token is not live")]
    Unauthorized,
    #[error("token lacks the required scope {0:?}")]
    InsufficientScope(ApiScope),
    #[error("requested object is outside this token's subject scoping")]
    OutOfScopeObject,
    #[error("no eligible implementation could satisfy this capability invocation")]
    NoEligibleImplementation,
    /// docs/26's own named "Rate/quota enforcement... no algorithm given" gap, closed with a
    /// real, per-caller fixed-window counter -- see [`crate::gateway::ApiGateway::
    /// check_rate_limit`]'s own doc comment for the algorithm.
    #[error("rate limit exceeded for this token's current window")]
    RateLimited,
    #[error("this action was assessed as {0:?} and requires explicit confirmation before it can proceed")]
    ConfirmationRequired(InterventionLevel),
    /// docs/23 §Pseudocode's `reconcile_ensemble`'s `EscalateToHuman` case, real for the first
    /// time: a real, dispatched verifying implementation disagreed with the primary and this
    /// crate has no designated tiebreaker to consult — both real outputs are preserved here
    /// rather than one being silently discarded, matching docs/23's own "-> 18, 01 §9" routing of
    /// an unresolvable ensemble disagreement to a human, not to a silently-chosen winner.
    #[error(
        "implementation {primary_impl} and its verifying implementation {alternative_impl} \
         disagreed, and there is no designated tiebreaker to consult"
    )]
    EnsembleDisagreement {
        primary_impl: PluginId,
        primary_output: serde_json::Value,
        alternative_impl: PluginId,
        alternative_output: serde_json::Value,
    },
    #[error("security risk-assessment error: {0}")]
    Security(#[from] hyperion_security::SecurityError),
    #[error("intent engine error: {0}")]
    Intent(#[from] hyperion_intent::IntentError),
    #[error("memory engine error: {0}")]
    Memory(#[from] hyperion_memory::MemoryError),
    #[error("knowledge graph error: {0}")]
    Graph(#[from] hyperion_knowledge_graph::GraphError),
    #[error("explainability error: {0}")]
    Explainability(#[from] hyperion_explainability::ExplainabilityError),
    #[error("context engine error: {0}")]
    Context(#[from] hyperion_context::ContextError),
    #[error("model router error: {0}")]
    ModelRouter(#[from] hyperion_model_router::ModelRouterError),
    #[error("observability error: {0}")]
    Observability(#[from] hyperion_observability::ObservabilityError),
}
