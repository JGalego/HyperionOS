use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hyperion_capability::{CapabilityMonitor, CapabilityToken, TokenId};
use hyperion_explainability::{ControlState, ExplanationStore};
use hyperion_intent::IntentEngine;
use hyperion_knowledge_graph::{GraphQuery, KnowledgeGraph, NodeId, QueryHit};
use hyperion_memory::{ErasureReceipt, MemoryEngine, MemoryFilter};
use hyperion_plugin_framework::PluginRegistry;

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
    scope_grants: Mutex<HashMap<TokenId, HashSet<ApiScope>>>,
    next_action_id: AtomicU64,
}

impl ApiGateway {
    pub fn new(
        intent: Arc<IntentEngine>,
        memory: Arc<MemoryEngine>,
        graph: Arc<KnowledgeGraph>,
        registry: Arc<PluginRegistry>,
        explainability: Arc<ExplanationStore>,
    ) -> Self {
        ApiGateway {
            intent,
            memory,
            graph,
            registry,
            explainability,
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

    /// docs/26 §4's `invokeCapability`: look up the Contract's competing
    /// implementations, pick the best (see this crate's doc comment on
    /// the deferred real Model Router selection), dispatch, and record an
    /// Explanation — explain-then-commit, per
    /// [18 — Explainability & Trust](../18-explainability-and-trust.md).
    /// On dispatch failure, retries against the next-best candidate
    /// before giving up with [`ApiError::NoEligibleImplementation`],
    /// matching the doc's own (partially-specified) fallback loop.
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
        let mut candidates = entry.implementations.clone();
        candidates.sort_by(|a, b| {
            b.quality_score
                .partial_cmp(&a.quality_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if candidates.is_empty() {
            return Err(ApiError::NoEligibleImplementation);
        }

        let action_id = self.next_action_id.fetch_add(1, Ordering::Relaxed);
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
        self.explainability
            .transition(monitor, token, explanation_id, ControlState::Executing)?;

        for candidate in &candidates {
            if let Ok(outputs) = hyperion_agent_runtime::dispatch_stub_capability(
                &request.contract_id,
                &request.inputs,
            ) {
                self.explainability.transition(
                    monitor,
                    token,
                    explanation_id,
                    ControlState::Completed,
                )?;
                return Ok(InvokeResponse {
                    outputs,
                    implementation_used: candidate.plugin_id,
                    explanation_id,
                });
            }
        }

        self.explainability
            .transition(monitor, token, explanation_id, ControlState::RolledBack)?;
        Err(ApiError::NoEligibleImplementation)
    }
}
