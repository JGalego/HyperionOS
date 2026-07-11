use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hyperion_ai_runtime::{CapabilityContract, InferenceRequest, LocalAiRuntime};
use hyperion_capability::{CapabilityMonitor, CapabilityToken, TokenId};
use hyperion_context::{Budget, ContextBundle, ContextEngine, Scope};
use hyperion_explainability::{ControlState, ExplanationStore, ReasoningStep};
use hyperion_intent::IntentEngine;
use hyperion_knowledge_graph::{GraphQuery, KnowledgeGraph, NodeId, QueryHit};
use hyperion_memory::{ErasureReceipt, MemoryEngine, MemoryFilter};
use hyperion_model_router::{ImplKind, ModelRouter};
use hyperion_observability::{AuditAction, AuditLedger, AuditPayload, PrincipalRef};
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
    /// The same `LocalAiRuntime` instance `model_router` was itself built with (see this
    /// struct's own `new` doc comment on why a second, disconnected instance would be wrong) --
    /// `model_router.route()` only ever *decides* which candidate to use; this is what actually
    /// runs real inference (M8) once `invoke_capability` dispatches to a `LocalSmallModel`/
    /// `LocalLargeModel` candidate.
    ai_runtime: Arc<LocalAiRuntime>,
    /// Backs the real routing-decision audit trail `invoke_capability`
    /// now writes — see this crate's doc comment.
    audit: Arc<AuditLedger>,
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
    /// `ai_runtime` is the same instance already threaded into
    /// `model_router`'s own construction, for the identical reason.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        intent: Arc<IntentEngine>,
        memory: Arc<MemoryEngine>,
        graph: Arc<KnowledgeGraph>,
        registry: Arc<PluginRegistry>,
        explainability: Arc<ExplanationStore>,
        model_router: Arc<ModelRouter>,
        context: Arc<ContextEngine>,
        ai_runtime: Arc<LocalAiRuntime>,
    ) -> Self {
        let recovery = Arc::new(RecoveryService::new(graph.clone()));
        let audit = Arc::new(AuditLedger::new());
        ApiGateway {
            intent,
            memory,
            graph,
            registry,
            explainability,
            model_router,
            recovery,
            context,
            ai_runtime,
            audit,
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

    /// Queryable proof that [`Self::invoke_capability`] really appends a
    /// real `hyperion_observability::AuditPayload::ModelRouting` entry
    /// per routing decision, rather than computing and discarding the
    /// `Rationale` — see this crate's doc comment.
    pub fn audit_query(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        filter: impl Fn(&hyperion_observability::AuditLogEntry) -> bool,
    ) -> Result<Vec<hyperion_observability::AuditLogEntry>, ApiError> {
        Ok(self.audit.query(monitor, token, filter)?)
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
            self.model_router
                .register_implementation(monitor, token, router_descriptor)?;
            self.model_router.set_rollout_stage(
                monitor,
                token,
                impl_id,
                hyperion_model_router::RolloutStage::Ga,
            )?;
        }

        let invocation = build_invocation(
            &request.contract_id,
            consequence_tier_for(risk_assessment.intervention_level),
        );
        let decision = self.model_router.route(&invocation);
        self.audit.append(
            monitor,
            token,
            PrincipalRef::Capability(token.token_id().0),
            AuditAction::ModelRouting,
            Some(request.contract_id.clone()),
            AuditPayload::ModelRouting(decision.rationale.clone()),
            now,
        )?;
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
            match self.dispatch_one(monitor, token, *impl_id, &request) {
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

    /// M8's real router-to-execution wiring: a candidate the Model Router itself registered as
    /// `LocalSmallModel`/`LocalLargeModel` now really runs through `self.ai_runtime.infer(...)`
    /// (a real Candle backend when one is configured -- see `hyperion-ai-runtime`'s own docs on
    /// [`hyperion_ai_runtime::MockBackend`] vs. a real [`hyperion_ai_runtime::InferenceBackend`]
    /// impl) instead of the stub dispatch every other candidate kind still uses. Without this,
    /// `model_router.route()` choosing a local-model candidate would never actually be
    /// distinguishable from any other kind: nothing before M8 ever called `ai_runtime.infer` from
    /// any production code path (only this crate's own unit tests did).
    fn dispatch_one(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        impl_id: hyperion_model_router::ImplId,
        request: &InvokeRequest,
    ) -> Result<serde_json::Value, String> {
        let local_model_class = self
            .model_router
            .descriptor(impl_id)
            .and_then(|descriptor| {
                let is_local_model = matches!(
                    descriptor.kind,
                    ImplKind::LocalSmallModel | ImplKind::LocalLargeModel
                );
                is_local_model.then_some(descriptor.model_class).flatten()
            });

        let Some(class) = local_model_class else {
            return hyperion_agent_runtime::dispatch_stub_capability(
                &request.contract_id,
                &request.inputs,
            );
        };

        // No existing spec pins an exact latency budget or prompt-construction convention for
        // this call site; a generous fixed budget avoids `infer`'s battery/latency-driven variant
        // downgrade firing for a reason unrelated to this specific call, and folding the contract
        // id into the prompt at least tells the model what it's being asked to do.
        let contract = CapabilityContract {
            latency_budget_ms: 5_000,
            always_on: false,
        };
        let inference_request = InferenceRequest {
            prompt: format!("{}: {}", request.contract_id, request.inputs),
        };
        self.ai_runtime
            .infer(monitor, token, class, &contract, &inference_request)
            .map(|result| serde_json::json!({ "text": result.text }))
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    //! `dispatch_one`'s real-inference branch is tested here, directly against this module's
    //! private field, rather than through `invoke_capability` (this crate's other tests'
    //! convention) -- `invoke_capability` always re-derives every candidate's
    //! `ImplementationDescriptor` from the Plugin Framework registry via
    //! [`crate::router_bridge::to_router_descriptor`], which cannot produce a `Some(ModelClass)`
    //! today (see that function's own doc comment: the Plugin Framework's manifest shape has no
    //! `ModelClass`-equivalent field to derive one from). That is a real, separate, documented gap
    //! this milestone doesn't close; testing `dispatch_one` directly proves the wiring this
    //! milestone *does* add is real and correct, independent of it.

    use super::*;
    use hyperion_ai_runtime::{
        sign, MockBackend, ModelClass, ModelDescriptor, Precision, QuantizedVariant,
    };
    use hyperion_capability::RightsMask;
    use hyperion_crypto::Keystore;
    use hyperion_model_router::{
        CostModel, ImplId, ImplementationDescriptor, PrivacyTier, RolloutStage,
    };

    fn gateway_with_registered_slm() -> (CapabilityMonitor, CapabilityToken, ApiGateway, ImplId) {
        let mut monitor = CapabilityMonitor::new();
        let token = monitor.mint_root(
            RightsMask::all(),
            hyperion_capability::TrustBoundaryId(1),
            None,
        );
        let dir = tempfile::tempdir().unwrap();
        let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
        let context = Arc::new(ContextEngine::new(graph.clone()));
        let intent = Arc::new(IntentEngine::new(graph.clone(), context.clone()));
        let memory = Arc::new(MemoryEngine::new(graph.clone()));
        let registry = Arc::new(PluginRegistry::new());
        let explainability = Arc::new(ExplanationStore::new());
        let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));
        let model_router = Arc::new(ModelRouter::new(ai_runtime.clone()));

        let key_dir = tempfile::tempdir().unwrap();
        let keystore = Keystore::open_or_create(&key_dir.path().join("device.key")).unwrap();

        let mut model_descriptor = ModelDescriptor {
            model_id: 1,
            class: ModelClass::Slm,
            variants: vec![QuantizedVariant {
                precision: Precision::Fp16,
                footprint_mb: 500,
                expected_tokens_per_sec: 40.0,
            }],
            signature: None,
        };
        model_descriptor.signature = Some(sign(&model_descriptor, &keystore));
        ai_runtime
            .register_model(model_descriptor, &keystore.verifying_key())
            .unwrap();

        let impl_id = ImplId(1);
        model_router
            .register_implementation(
                &monitor,
                &token,
                ImplementationDescriptor {
                    impl_id,
                    capability_id: "intent.parse".to_string(),
                    kind: ImplKind::LocalSmallModel,
                    model_class: Some(ModelClass::Slm),
                    privacy_tier: PrivacyTier::Local,
                    cost_model: CostModel::Free,
                    quality_profile: std::collections::HashMap::new(),
                    declared_latency_ms: 100,
                    rollout_stage: RolloutStage::Ga,
                },
            )
            .unwrap();

        let gateway = ApiGateway::new(
            intent,
            memory,
            graph,
            registry,
            explainability,
            model_router,
            context,
            ai_runtime,
        );
        (monitor, token, gateway, impl_id)
    }

    #[test]
    fn a_local_small_model_candidate_really_dispatches_through_real_inference_not_the_stub() {
        let (monitor, token, gateway, impl_id) = gateway_with_registered_slm();
        let request = InvokeRequest {
            contract_id: "intent.parse".to_string(),
            inputs: serde_json::json!({"utterance": "launch my startup"}),
            agent_id: 1,
            intent_id: 1,
            risk: crate::types::RiskHints::default(),
            confirmed: false,
        };

        let outputs = gateway
            .dispatch_one(&monitor, &token, impl_id, &request)
            .expect("a registered, resident Slm model must really dispatch");

        // MockBackend's real, distinctive echo shape -- if this ever fell through to the stub
        // dispatch instead, `outputs` would carry `{"results": [...]}`
        // (`hyperion_agent_runtime::stubs::dispatch`'s own shape for an unknown capability_id),
        // never this text.
        let text = outputs
            .get("text")
            .and_then(|v| v.as_str())
            .expect("real inference must produce a real text field");
        assert!(
            text.contains("mock model") && text.contains("intent.parse"),
            "expected MockBackend's real echo of the real prompt, got: {text:?}"
        );
    }

    #[test]
    fn a_cloud_api_candidate_still_dispatches_through_the_real_stub_unchanged() {
        let (monitor, token, gateway, _impl_id) = gateway_with_registered_slm();
        // No model_router registration at all for this impl_id -- `descriptor(..)` returns
        // `None`, so `dispatch_one` must fall back to the stub path exactly as it always has.
        let unregistered = ImplId(999);
        let request = InvokeRequest {
            contract_id: "web.search".to_string(),
            inputs: serde_json::json!({"query": "hyperion os"}),
            agent_id: 1,
            intent_id: 1,
            risk: crate::types::RiskHints::default(),
            confirmed: false,
        };

        let outputs = gateway
            .dispatch_one(&monitor, &token, unregistered, &request)
            .expect("the real stub capability must still handle a non-local-model candidate");
        assert!(
            outputs.get("results").is_some(),
            "expected the real web.search stub's own shape, got: {outputs:?}"
        );
    }
}
