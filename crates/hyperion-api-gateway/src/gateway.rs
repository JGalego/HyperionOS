use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hyperion_capability::{CapabilityMonitor, CapabilityToken, TokenId};
use hyperion_context::{Budget, ContextBundle, ContextEngine, Scope};
use hyperion_explainability::{ControlState, ExplanationStore, ReasoningStep};
use hyperion_intent::IntentEngine;
use hyperion_knowledge_graph::{GraphQuery, KnowledgeGraph, NodeId, QueryHit};
use hyperion_memory::{ErasureReceipt, MemoryEngine, MemoryFilter};
use hyperion_model_router::ModelRouter;
use hyperion_plugin_framework::PluginRegistry;
use hyperion_recovery::RecoveryService;
use hyperion_security::{InterventionLevel, PendingAction};

use crate::router_bridge::{
    build_invocation, consequence_tier_for, to_confidence_and_alternatives, to_router_descriptor,
};
use crate::types::{
    ApiError, ApiScope, InvokeRequest, InvokeResponse, SubmitIntentRequest, SubmitIntentResponse,
};

/// docs/26 — the API Gateway: "a thin, uniform gateway in front of five
/// subsystem servers." See this crate's doc comment for the full real/
/// deferred split. Owns no business logic beyond auth, routing, and (for
/// Capability Invocation only) orchestrating selection + dispatch +
/// explainability recording as one bundled unit.
pub struct ApiGateway {
    intent: Arc<IntentEngine>,
    memory: Arc<MemoryEngine>,
    graph: Arc<KnowledgeGraph>,
    registry: Arc<PluginRegistry>,
    explainability: Arc<ExplanationStore>,
    model_router: Arc<ModelRouter>,
    recovery: Arc<RecoveryService>,
    context: Arc<ContextEngine>,
    scope_grants: Mutex<HashMap<TokenId, HashSet<ApiScope>>>,
    next_action_id: AtomicU64,
}

impl ApiGateway {
    /// `context` is taken as a parameter, not built internally like
    /// [`RecoveryService`] — it's the *same* `ContextEngine` instance the
    /// caller already threads into `hyperion_intent::IntentEngine::new`,
    /// and a second, disconnected instance would silently diverge (its
    /// own working-set hysteresis, its own bundle history) rather than
    /// sharing state with the one Intent grounding actually uses.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        intent: Arc<IntentEngine>,
        memory: Arc<MemoryEngine>,
        graph: Arc<KnowledgeGraph>,
        registry: Arc<PluginRegistry>,
        explainability: Arc<ExplanationStore>,
        model_router: Arc<ModelRouter>,
        context: Arc<ContextEngine>,
    ) -> Self {
        let recovery = Arc::new(RecoveryService::new(graph.clone()));
        ApiGateway {
            intent,
            memory,
            graph,
            registry,
            explainability,
            model_router,
            recovery,
            context,
            scope_grants: Mutex::new(HashMap::new()),
            next_action_id: AtomicU64::new(1),
        }
    }

    /// docs/26 §3's scope grant — the gateway "mints no separate identity
    /// model, it re-checks the same tokens the kernel issues"; scopes are
    /// this gateway's own bookkeeping on top of that real token, keyed by
    /// the token's real identity (`TokenId`), the same pattern
    /// `hyperion-netstack`'s `DomainEgressGrant` already established.
    pub fn grant_scopes(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        scopes: HashSet<ApiScope>,
    ) -> Result<(), ApiError> {
        if !monitor.is_live(token) {
            return Err(ApiError::Unauthorized);
        }
        self.scope_grants
            .lock()
            .unwrap()
            .insert(token.token_id(), scopes);
        Ok(())
    }

    /// docs/26 §3/§4's two-step check, in order: live-token verify, then
    /// scope match. "Not a separate session/identity layer" — a token
    /// that fails `is_live` (expired, or revoked and re-checked live via
    /// its generation) is `Unauthorized` before scope is even consulted.
    fn authorize(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        scope: ApiScope,
    ) -> Result<(), ApiError> {
        if !monitor.is_live(token) {
            return Err(ApiError::Unauthorized);
        }
        let grants = self.scope_grants.lock().unwrap();
        let granted = grants
            .get(&token.token_id())
            .ok_or(ApiError::InsufficientScope(scope))?;
        if !granted.contains(&scope) {
            return Err(ApiError::InsufficientScope(scope));
        }
        Ok(())
    }

    /// docs/26 §2's Intent API, backed by the real `hyperion-intent`
    /// engine — not a mock.
    pub fn submit_intent(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        request: SubmitIntentRequest,
    ) -> Result<SubmitIntentResponse, ApiError> {
        self.authorize(monitor, token, ApiScope::IntentSubmit)?;
        let outcome = self.intent.handle_utterance(
            monitor,
            token,
            &request.utterance,
            &request.session_id,
        )?;
        Ok(outcome.into())
    }

    /// docs/26 §2's Knowledge Graph API `POST /kg/query`, backed by the
    /// real `hyperion-knowledge-graph`.
    pub fn kg_query(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        query: &GraphQuery,
    ) -> Result<Vec<QueryHit>, ApiError> {
        self.authorize(monitor, token, ApiScope::KgQuery)?;
        Ok(self.graph.query(monitor, token, query)?)
    }

    /// docs/26 §2's Knowledge Graph API `POST /kg/write`.
    pub fn kg_write(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        object_type: &str,
        metadata: serde_json::Value,
    ) -> Result<NodeId, ApiError> {
        self.authorize(monitor, token, ApiScope::KgWrite)?;
        Ok(self
            .graph
            .put_node(monitor, token, None, object_type, None, metadata)?)
    }

    /// docs/26 §2's Memory API `POST /memory`.
    pub fn memory_write(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        fact: serde_json::Value,
    ) -> Result<(NodeId, NodeId), ApiError> {
        self.authorize(monitor, token, ApiScope::MemoryWrite)?;
        Ok(self.memory.remember_explicit(monitor, token, fact, None)?)
    }

    /// docs/26 §3's explicit carve-out: "Memory export/erase are
    /// deliberately *not* gated behind any Capability's permission — a
    /// user's own export/erase always succeeds regardless of installed-
    /// Capability declarations." No `authorize()` call here, by design —
    /// this bypasses the scope check entirely, not merely widens it.
    pub fn memory_erase(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: NodeId,
        cascade: bool,
    ) -> Result<ErasureReceipt, ApiError> {
        Ok(self.memory.erase(monitor, token, id, cascade)?)
    }

    /// Same carve-out as [`Self::memory_erase`].
    pub fn memory_export(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        filter: &MemoryFilter,
    ) -> Result<serde_json::Value, ApiError> {
        Ok(self.memory.export(monitor, token, filter)?)
    }

    /// docs/26 §2's Context API `POST /context/assemble`, backed by the
    /// real `hyperion-context` engine — the fourth of the gateway's five
    /// subsystem routes, previously left unwired (see this crate's doc
    /// comment on why it was a deliberate follow-up rather than a rushed
    /// fourth alongside Intent/KG/Memory).
    pub fn context_assemble(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        scope: &Scope,
        budget: Budget,
    ) -> Result<ContextBundle, ApiError> {
        self.authorize(monitor, token, ApiScope::ContextAssemble)?;
        Ok(self.context.assemble(monitor, token, scope, budget)?)
    }

    /// docs/26 §4's `invokeCapability`: look up the Contract's competing
    /// implementations, run the real `hyperion-security` Risk-Assessment
    /// Engine against the request's [`crate::types::RiskHints`] (denying
    /// with [`ApiError::ConfirmationRequired`] if it demands confirmation
    /// the caller hasn't given), ask the real `hyperion-model-router`
    /// which implementation wins (via [`crate::router_bridge`]'s adapter
    /// — see this crate's doc comment for exactly what that bridge does
    /// and doesn't carry yet), dispatch, and record an Explanation —
    /// explain-then-commit, per
    /// [18 — Explainability & Trust](../18-explainability-and-trust.md),
    /// including the risk rationale and the routing decision's own
    /// `chosen_reason` as real reasoning steps, the routing decision's
    /// real winning composite score and every considered/excluded
    /// candidate as a real Confidence/Alternatives pair (via
    /// [`crate::router_bridge::to_confidence_and_alternatives`]), and a
    /// real recovery-point undo reference when one was created.
    /// On dispatch failure, reports the failure to the Model Router's
    /// real circuit breaker and retries against the next entry in its
    /// `fallback_chain` before giving up with
    /// [`ApiError::NoEligibleImplementation`], matching docs/26's own
    /// (partially-specified) fallback loop.
    pub fn invoke_capability(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        request: InvokeRequest,
        now: u64,
    ) -> Result<InvokeResponse, ApiError> {
        self.authorize(monitor, token, ApiScope::CapabilityInvoke)?;

        let entry = self
            .registry
            .query(&request.contract_id)
            .ok_or(ApiError::NoEligibleImplementation)?;
        if entry.implementations.is_empty() {
            return Err(ApiError::NoEligibleImplementation);
        }

        let action_id = self.next_action_id.fetch_add(1, Ordering::Relaxed);

        // docs/15's real Risk-Assessment Engine, run synchronously before
        // dispatch — see this crate's doc comment for exactly what it
        // does and doesn't derive.
        let pending_action = PendingAction {
            action_id,
            object_refs: request.risk.object_refs.clone(),
            scope_size: request.risk.scope_size,
            reversible: request.risk.reversible,
            sensitivity: request.risk.sensitivity,
            intent_confidence: request.risk.intent_confidence,
            corroboration: request.risk.corroboration,
            provenance: request.risk.provenance.clone(),
        };
        let risk_assessment = hyperion_security::assess_and_prepare(
            monitor,
            token,
            &self.recovery,
            &pending_action,
            now,
        )?;
        if risk_assessment.intervention_level >= InterventionLevel::RequireExplicitConfirm
            && !request.confirmed
        {
            return Err(ApiError::ConfirmationRequired(
                risk_assessment.intervention_level,
            ));
        }

        // Sync the Model Router's view of this capability_id's candidates
        // with the Plugin Framework's current registry state. Re-done on
        // every call rather than incrementally maintained — correctness
        // over efficiency, appropriate at this scale (see this crate's
        // doc comment).
        for plugin_descriptor in &entry.implementations {
            let router_descriptor = to_router_descriptor(plugin_descriptor, &request.contract_id);
            let impl_id = router_descriptor.impl_id;
            self.model_router.register_implementation(router_descriptor);
            self.model_router
                .set_rollout_stage(impl_id, hyperion_model_router::RolloutStage::Ga);
        }

        let invocation = build_invocation(
            &request.contract_id,
            consequence_tier_for(risk_assessment.intervention_level),
        );
        let decision = self.model_router.route(&invocation);
        if decision.chosen.is_none() {
            return Err(ApiError::NoEligibleImplementation);
        }

        let explanation_id = self.explainability.begin(
            monitor,
            token,
            action_id,
            request.intent_id,
            request.agent_id,
            &request.contract_id,
            vec![],
            now,
        )?;
        self.explainability.append_step(
            monitor,
            token,
            explanation_id,
            ReasoningStep {
                step_index: 0,
                description: risk_assessment.rationale.clone(),
                capability_ref: Some(request.contract_id.clone()),
                inputs_ref: request.risk.object_refs.clone(),
                output_ref: None,
            },
            vec![],
        )?;
        self.explainability.append_step(
            monitor,
            token,
            explanation_id,
            ReasoningStep {
                step_index: 1,
                description: decision.rationale.chosen_reason.clone(),
                capability_ref: Some(request.contract_id.clone()),
                inputs_ref: Vec::new(),
                output_ref: None,
            },
            vec![],
        )?;
        let (confidence, alternatives) = to_confidence_and_alternatives(&decision);
        self.explainability.set_confidence(
            monitor,
            token,
            explanation_id,
            confidence,
            alternatives,
        )?;
        if let Some(recovery_point) = risk_assessment.recovery_point_ref {
            self.explainability
                .attach_undo_ref(monitor, token, explanation_id, recovery_point)?;
        }
        self.explainability
            .transition(monitor, token, explanation_id, ControlState::Executing)?;

        for impl_id in &decision.fallback_chain {
            match hyperion_agent_runtime::dispatch_stub_capability(
                &request.contract_id,
                &request.inputs,
            ) {
                Ok(outputs) => {
                    self.model_router.report_outcome(*impl_id, true);
                    self.explainability.transition(
                        monitor,
                        token,
                        explanation_id,
                        ControlState::Completed,
                    )?;
                    return Ok(InvokeResponse {
                        outputs,
                        implementation_used: impl_id.0,
                        explanation_id,
                    });
                }
                Err(_) => {
                    self.model_router.report_outcome(*impl_id, false);
                }
            }
        }

        self.explainability
            .transition(monitor, token, explanation_id, ControlState::RolledBack)?;
        Err(ApiError::NoEligibleImplementation)
    }
}
