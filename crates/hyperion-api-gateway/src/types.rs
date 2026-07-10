use hyperion_explainability::ExplanationId;
use hyperion_intent::HandleOutcome;
use hyperion_knowledge_graph::NodeId;
use hyperion_plugin_framework::PluginId;

/// docs/26 §2's per-endpoint scope strings (`"intent:submit"`,
/// `"memory:erase"`, ...), as a closed enum instead of open strings —
/// this crate's caller can't typo a scope name past the compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ApiScope {
    IntentSubmit,
    MemoryWrite,
    KgQuery,
    KgWrite,
    CapabilityInvoke,
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
}

impl From<HandleOutcome> for SubmitIntentResponse {
    fn from(outcome: HandleOutcome) -> Self {
        match outcome {
            HandleOutcome::Submitted(id) => SubmitIntentResponse::Submitted { intent_id: id },
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

/// docs/26 §2's Capability Invocation API `InvokeRequest` — never names
/// an Implementation, only a `contractId`; selection is delegated
/// entirely to the registry + this crate's placeholder selection (see
/// this crate's doc comment on the deferred real Model Router wiring).
#[derive(Debug, Clone)]
pub struct InvokeRequest {
    pub contract_id: String,
    pub inputs: serde_json::Value,
    pub agent_id: u64,
    pub intent_id: u64,
}

/// docs/26 §2's `InvokeResponse`.
#[derive(Debug, Clone)]
pub struct InvokeResponse {
    pub outputs: serde_json::Value,
    pub implementation_used: PluginId,
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
    #[error("intent engine error: {0}")]
    Intent(#[from] hyperion_intent::IntentError),
    #[error("memory engine error: {0}")]
    Memory(#[from] hyperion_memory::MemoryError),
    #[error("knowledge graph error: {0}")]
    Graph(#[from] hyperion_knowledge_graph::GraphError),
    #[error("explainability error: {0}")]
    Explainability(#[from] hyperion_explainability::ExplainabilityError),
}
