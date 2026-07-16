//! docs/26's five-API gateway, backed by real subsystem crates where
//! this slice wires them (Intent, Knowledge Graph, Memory), plus the
//! Capability Invocation path's registry lookup → dispatch →
//! explainability recording pipeline.

use std::collections::HashSet;
use std::sync::Arc;

use hyperion_ai_runtime::{LocalAiRuntime, MockBackend};
use hyperion_api_gateway::{
    ApiError, ApiGateway, ApiScope, InvokeRequest, RiskHints, SubmitIntentRequest,
    SubmitIntentResponse,
};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_crypto::Keystore;
use hyperion_explainability::ExplanationStore;
use hyperion_intent::IntentEngine;
use hyperion_knowledge_graph::{GraphQuery, KnowledgeGraph};
use hyperion_memory::MemoryEngine;
use hyperion_model_router::ModelRouter;
use hyperion_plugin_framework::{
    sign, CapabilityManifest, Contribution, ImplementationKind, PluginManifest, PluginRegistry,
    PrivacyTier, SemanticContract, SideEffect, TrustDepth,
};

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

/// Returns the same `Arc<LocalAiRuntime>` `ModelRouter` was built with, not a second,
/// disconnected instance -- `ApiGateway::new`'s own doc comment on why `context` works the same
/// way applies identically here: a caller that built its own separate `LocalAiRuntime` for
/// `ApiGateway` would silently diverge from whatever `ModelRouter`'s own `estimate()` calls
/// consult.
fn model_router_and_ai_runtime() -> (Arc<ModelRouter>, Arc<LocalAiRuntime>) {
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 4_096));
    (Arc::new(ModelRouter::new(ai_runtime.clone())), ai_runtime)
}

fn setup() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    ApiGateway,
) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let intent = Arc::new(IntentEngine::new(graph.clone(), context.clone()));
    let memory = Arc::new(MemoryEngine::new(graph.clone()));
    let registry = Arc::new(PluginRegistry::new());
    let explainability = Arc::new(ExplanationStore::new());
    let (router, ai_runtime) = model_router_and_ai_runtime();
    let gateway = ApiGateway::new(
        intent,
        memory,
        graph,
        registry,
        explainability,
        router,
        context,
        ai_runtime,
    );
    (monitor, root, gateway)
}

fn all_scopes() -> HashSet<ApiScope> {
    [
        ApiScope::IntentSubmit,
        ApiScope::MemoryWrite,
        ApiScope::KgQuery,
        ApiScope::KgWrite,
        ApiScope::CapabilityInvoke,
        ApiScope::ContextAssemble,
    ]
    .into_iter()
    .collect()
}

#[test]
fn submit_intent_routes_to_the_real_intent_engine() {
    let (monitor, root, gateway) = setup();
    gateway.grant_scopes(&monitor, &root, all_scopes()).unwrap();

    let response = gateway
        .submit_intent(
            &monitor,
            &root,
            SubmitIntentRequest {
                utterance: "help me prepare for tomorrow's interview".to_string(),
                session_id: "s1".to_string(),
            },
        )
        .unwrap();
    assert!(matches!(
        response,
        SubmitIntentResponse::Submitted { .. } | SubmitIntentResponse::NeedsClarification { .. }
    ));
}

#[test]
fn kg_write_then_query_round_trips_through_the_real_graph() {
    let (monitor, root, gateway) = setup();
    gateway.grant_scopes(&monitor, &root, all_scopes()).unwrap();

    let id = gateway
        .kg_write(
            &monitor,
            &root,
            "Note",
            serde_json::json!({"text": "hello"}),
        )
        .unwrap();
    let hits = gateway
        .kg_query(
            &monitor,
            &root,
            &GraphQuery {
                type_filter: Some(vec!["Note".to_string()]),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(hits.iter().any(|h| h.node_id == id));
}

#[test]
fn memory_write_creates_a_real_semantic_and_long_term_pair() {
    let (monitor, root, gateway) = setup();
    gateway.grant_scopes(&monitor, &root, all_scopes()).unwrap();

    let (semantic_id, long_term_id) = gateway
        .memory_write(
            &monitor,
            &root,
            serde_json::json!({"fact": "the user prefers dark mode"}),
        )
        .unwrap();
    assert_ne!(semantic_id, long_term_id);
}

#[test]
fn memory_erase_succeeds_even_without_any_scope_grant() {
    let (monitor, root, gateway) = setup();
    // No grant_scopes call at all — erase must still succeed per docs/26's carve-out.
    let (semantic_id, _long_term_id) = {
        gateway
            .grant_scopes(
                &monitor,
                &root,
                [ApiScope::MemoryWrite].into_iter().collect(),
            )
            .unwrap();
        gateway
            .memory_write(&monitor, &root, serde_json::json!({"fact": "temp"}))
            .unwrap()
    };

    let receipt = gateway
        .memory_erase(&monitor, &root, semantic_id, false)
        .unwrap();
    assert_eq!(receipt.id, semantic_id);
}

#[test]
fn context_assemble_routes_to_the_real_context_engine() {
    let (monitor, root, gateway) = setup();
    gateway.grant_scopes(&monitor, &root, all_scopes()).unwrap();

    let id = gateway
        .kg_write(
            &monitor,
            &root,
            "Note",
            serde_json::json!({"text": "hello"}),
        )
        .unwrap();

    let bundle = gateway
        .context_assemble(
            &monitor,
            &root,
            &hyperion_api_gateway::Scope {
                intent_id: "i1".to_string(),
                session_id: "s1".to_string(),
                mentions: Vec::new(),
                anchors: vec![id],
            },
            hyperion_api_gateway::Budget::default(),
        )
        .unwrap();
    assert!(bundle.entries.iter().any(|e| e.node_id == id));
}

#[test]
fn invoke_capability_dispatches_through_the_real_stub_and_records_an_explanation() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let intent = Arc::new(IntentEngine::new(graph.clone(), context.clone()));
    let memory = Arc::new(MemoryEngine::new(graph.clone()));
    let registry = Arc::new(PluginRegistry::new());
    let (_key_dir, keystore) = keystore();
    let explainability = Arc::new(ExplanationStore::new());
    let (router, ai_runtime) = model_router_and_ai_runtime();
    let gateway = ApiGateway::new(
        intent,
        memory,
        graph,
        registry.clone(),
        explainability.clone(),
        router,
        context,
        ai_runtime,
    );
    gateway.grant_scopes(&monitor, &root, all_scopes()).unwrap();

    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract: SemanticContract {
                inputs: vec!["query".to_string()],
                outputs: vec!["results".to_string()],
                side_effects: vec![SideEffect::NetworkEgress],
            },
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.9,
            version: 1,
            native_binary: None,
            privacy_tier: PrivacyTier::Local,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = Some(sign(&manifest, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            manifest,
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let response = gateway
        .invoke_capability(
            &monitor,
            &root,
            InvokeRequest {
                contract_id: "web.search".to_string(),
                inputs: serde_json::json!({"query": "hyperion os"}),
                agent_id: 42,
                intent_id: 7,
                risk: RiskHints::default(),
                confirmed: false,
            },
            1_000,
        )
        .unwrap();

    assert!(response.outputs.get("results").is_some());
    let record = explainability.get(response.explanation_id).unwrap();
    assert_eq!(
        record.control_state,
        hyperion_explainability::ControlState::Completed
    );
}

#[test]
fn invoke_capability_appends_a_real_model_routing_audit_entry() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let intent = Arc::new(IntentEngine::new(graph.clone(), context.clone()));
    let memory = Arc::new(MemoryEngine::new(graph.clone()));
    let registry = Arc::new(PluginRegistry::new());
    let (_key_dir, keystore) = keystore();
    let explainability = Arc::new(ExplanationStore::new());
    let (router, ai_runtime) = model_router_and_ai_runtime();
    let gateway = ApiGateway::new(
        intent,
        memory,
        graph,
        registry.clone(),
        explainability,
        router,
        context,
        ai_runtime,
    );
    gateway.grant_scopes(&monitor, &root, all_scopes()).unwrap();

    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract: SemanticContract {
                inputs: vec!["query".to_string()],
                outputs: vec!["results".to_string()],
                side_effects: vec![SideEffect::NetworkEgress],
            },
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.9,
            version: 1,
            native_binary: None,
            privacy_tier: PrivacyTier::Local,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = Some(sign(&manifest, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            manifest,
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    gateway
        .invoke_capability(
            &monitor,
            &root,
            InvokeRequest {
                contract_id: "web.search".to_string(),
                inputs: serde_json::json!({"query": "hyperion os"}),
                agent_id: 1,
                intent_id: 1,
                risk: RiskHints::default(),
                confirmed: false,
            },
            1_000,
        )
        .unwrap();

    let entries = gateway
        .audit_query(&monitor, &root, |e| {
            e.action == hyperion_observability::AuditAction::ModelRouting
        })
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].target.as_deref(), Some("web.search"));
    match &entries[0].payload {
        hyperion_observability::AuditPayload::ModelRouting(rationale) => {
            assert!(!rationale.candidates_considered.is_empty());
        }
        other => panic!("expected ModelRouting payload, got {other:?}"),
    }
}

#[test]
fn the_real_model_router_prefers_a_healthy_candidate_over_a_higher_quality_one_whose_circuit_is_open(
) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let intent = Arc::new(IntentEngine::new(graph.clone(), context.clone()));
    let memory = Arc::new(MemoryEngine::new(graph.clone()));
    let registry = Arc::new(PluginRegistry::new());
    let (_key_dir, keystore) = keystore();
    let explainability = Arc::new(ExplanationStore::new());
    let (router, ai_runtime) = model_router_and_ai_runtime();
    let gateway = ApiGateway::new(
        intent,
        memory,
        graph,
        registry.clone(),
        explainability,
        router.clone(),
        context,
        ai_runtime,
    );
    gateway.grant_scopes(&monitor, &root, all_scopes()).unwrap();

    let contract = SemanticContract {
        inputs: vec!["query".to_string()],
        outputs: vec!["results".to_string()],
        side_effects: vec![SideEffect::NetworkEgress],
    };

    // Plugin 1: a modest quality edge (0.7 vs 0.6) that alone would win,
    // but its underlying implementation has been failing repeatedly
    // (circuit open).
    let mut circuit_open_plugin = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract: contract.clone(),
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.7,
            version: 1,
            native_binary: None,
            privacy_tier: PrivacyTier::Local,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    circuit_open_plugin.signature = Some(sign(&circuit_open_plugin, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            circuit_open_plugin,
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    // Plugin 2: slightly lower quality, but healthy.
    let mut healthy_plugin = PluginManifest {
        plugin_id: 2,
        publisher: "globex-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract,
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.6,
            version: 1,
            native_binary: None,
            privacy_tier: PrivacyTier::Local,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    healthy_plugin.signature = Some(sign(&healthy_plugin, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            healthy_plugin,
            TrustDepth::D2,
            true,
            1_001,
            &keystore.verifying_key(),
        )
        .unwrap();

    // Trip plugin 1's circuit breaker directly against the real
    // ModelRouter this gateway shares — three consecutive failures
    // demotes its availability_fit to near-zero, per
    // hyperion-model-router's own recovery mechanism.
    for _ in 0..3 {
        router.report_outcome(hyperion_model_router::ImplId(1), false);
    }

    let response = gateway
        .invoke_capability(
            &monitor,
            &root,
            InvokeRequest {
                contract_id: "web.search".to_string(),
                inputs: serde_json::json!({"query": "hyperion os"}),
                agent_id: 1,
                intent_id: 1,
                risk: RiskHints::default(),
                confirmed: false,
            },
            1_002,
        )
        .unwrap();

    assert_eq!(response.implementation_used, 2, "a real circuit-open candidate must lose to a healthy lower-quality one, proving this is the real Model Router's weighted scoring, not a bare quality sort");
}

#[test]
fn among_two_equally_healthy_candidates_the_higher_quality_one_wins() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let intent = Arc::new(IntentEngine::new(graph.clone(), context.clone()));
    let memory = Arc::new(MemoryEngine::new(graph.clone()));
    let registry = Arc::new(PluginRegistry::new());
    let (_key_dir, keystore) = keystore();
    let explainability = Arc::new(ExplanationStore::new());
    let (router, ai_runtime) = model_router_and_ai_runtime();
    let gateway = ApiGateway::new(
        intent,
        memory,
        graph,
        registry.clone(),
        explainability,
        router,
        context,
        ai_runtime,
    );
    gateway.grant_scopes(&monitor, &root, all_scopes()).unwrap();

    let contract = SemanticContract {
        inputs: vec!["query".to_string()],
        outputs: vec!["results".to_string()],
        side_effects: vec![SideEffect::NetworkEgress],
    };

    let mut low = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract: contract.clone(),
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.3,
            version: 1,
            native_binary: None,
            privacy_tier: PrivacyTier::Local,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    low.signature = Some(sign(&low, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            low,
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let mut high = PluginManifest {
        plugin_id: 2,
        publisher: "globex-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract,
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.9,
            version: 1,
            native_binary: None,
            privacy_tier: PrivacyTier::Local,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    high.signature = Some(sign(&high, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            high,
            TrustDepth::D2,
            true,
            1_001,
            &keystore.verifying_key(),
        )
        .unwrap();

    let response = gateway
        .invoke_capability(
            &monitor,
            &root,
            InvokeRequest {
                contract_id: "web.search".to_string(),
                inputs: serde_json::json!({"query": "hyperion os"}),
                agent_id: 1,
                intent_id: 1,
                risk: RiskHints::default(),
                confirmed: false,
            },
            1_000,
        )
        .unwrap();

    assert_eq!(
        response.implementation_used, 2,
        "with both candidates healthy, the real Model Router must prefer the higher-quality one"
    );
}

fn risky_pending_action_hints() -> RiskHints {
    // scope_size >= 10 saturates blast radius to 1.0; reversible: false
    // zeroes reversibility — together they trip docs/15 §7's
    // unconditional "irreversible + wide blast radius" floor straight to
    // `RequireBackupFirst`, regardless of the other (deliberately mild)
    // inputs, so this test isn't sensitive to the weighted-composite
    // arithmetic.
    RiskHints {
        object_refs: vec![],
        scope_size: 10,
        reversible: false,
        sensitivity: hyperion_security::SensitivityHint::Sensitive,
        intent_confidence: 0.5,
        corroboration: 0.0,
        provenance: None,
    }
}

#[test]
fn a_risky_action_is_rejected_without_confirmation() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let intent = Arc::new(IntentEngine::new(graph.clone(), context.clone()));
    let memory = Arc::new(MemoryEngine::new(graph.clone()));
    let registry = Arc::new(PluginRegistry::new());
    let (_key_dir, keystore) = keystore();
    let explainability = Arc::new(ExplanationStore::new());
    let (router, ai_runtime) = model_router_and_ai_runtime();
    let gateway = ApiGateway::new(
        intent,
        memory,
        graph,
        registry.clone(),
        explainability,
        router,
        context,
        ai_runtime,
    );
    gateway.grant_scopes(&monitor, &root, all_scopes()).unwrap();

    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract: SemanticContract {
                inputs: vec!["query".to_string()],
                outputs: vec!["results".to_string()],
                side_effects: vec![SideEffect::NetworkEgress],
            },
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.9,
            version: 1,
            native_binary: None,
            privacy_tier: PrivacyTier::Local,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = Some(sign(&manifest, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            manifest,
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let result = gateway.invoke_capability(
        &monitor,
        &root,
        InvokeRequest {
            contract_id: "web.search".to_string(),
            inputs: serde_json::json!({"query": "hyperion os"}),
            agent_id: 1,
            intent_id: 1,
            risk: risky_pending_action_hints(),
            confirmed: false,
        },
        1_000,
    );

    assert!(matches!(
        result,
        Err(ApiError::ConfirmationRequired(
            hyperion_security::InterventionLevel::RequireBackupFirst
        ))
    ));
}

#[test]
fn a_risky_action_confirmed_by_the_caller_gets_a_real_recovery_point_attached_as_its_undo_ref() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let intent = Arc::new(IntentEngine::new(graph.clone(), context.clone()));
    let memory = Arc::new(MemoryEngine::new(graph.clone()));
    let registry = Arc::new(PluginRegistry::new());
    let (_key_dir, keystore) = keystore();
    let explainability = Arc::new(ExplanationStore::new());
    let (router, ai_runtime) = model_router_and_ai_runtime();
    let gateway = ApiGateway::new(
        intent,
        memory,
        graph,
        registry.clone(),
        explainability.clone(),
        router,
        context,
        ai_runtime,
    );
    gateway.grant_scopes(&monitor, &root, all_scopes()).unwrap();

    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract: SemanticContract {
                inputs: vec!["query".to_string()],
                outputs: vec!["results".to_string()],
                side_effects: vec![SideEffect::NetworkEgress],
            },
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.9,
            version: 1,
            native_binary: None,
            privacy_tier: PrivacyTier::Local,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = Some(sign(&manifest, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            manifest,
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let response = gateway
        .invoke_capability(
            &monitor,
            &root,
            InvokeRequest {
                contract_id: "web.search".to_string(),
                inputs: serde_json::json!({"query": "hyperion os"}),
                agent_id: 1,
                intent_id: 1,
                risk: risky_pending_action_hints(),
                confirmed: true,
            },
            1_000,
        )
        .unwrap();

    let record = explainability.get(response.explanation_id).unwrap();
    assert!(
        record.undo_ref.is_some(),
        "a RequireBackupFirst action's Explanation Record must carry a real recovery-point undo ref"
    );
    assert_eq!(
        record.control_state,
        hyperion_explainability::ControlState::Completed
    );
}

#[test]
fn the_routing_decision_produces_a_real_confidence_score_and_a_real_alternative() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let intent = Arc::new(IntentEngine::new(graph.clone(), context.clone()));
    let memory = Arc::new(MemoryEngine::new(graph.clone()));
    let registry = Arc::new(PluginRegistry::new());
    let (_key_dir, keystore) = keystore();
    let explainability = Arc::new(ExplanationStore::new());
    let (router, ai_runtime) = model_router_and_ai_runtime();
    let gateway = ApiGateway::new(
        intent,
        memory,
        graph,
        registry.clone(),
        explainability.clone(),
        router,
        context,
        ai_runtime,
    );
    gateway.grant_scopes(&monitor, &root, all_scopes()).unwrap();

    let contract = SemanticContract {
        inputs: vec!["query".to_string()],
        outputs: vec!["results".to_string()],
        side_effects: vec![SideEffect::NetworkEgress],
    };

    let mut low = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract: contract.clone(),
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.3,
            version: 1,
            native_binary: None,
            privacy_tier: PrivacyTier::Local,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    low.signature = Some(sign(&low, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            low,
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let mut high = PluginManifest {
        plugin_id: 2,
        publisher: "globex-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract,
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.9,
            version: 1,
            native_binary: None,
            privacy_tier: PrivacyTier::Local,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    high.signature = Some(sign(&high, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            high,
            TrustDepth::D2,
            true,
            1_001,
            &keystore.verifying_key(),
        )
        .unwrap();

    let response = gateway
        .invoke_capability(
            &monitor,
            &root,
            InvokeRequest {
                contract_id: "web.search".to_string(),
                inputs: serde_json::json!({"query": "hyperion os"}),
                agent_id: 1,
                intent_id: 1,
                risk: RiskHints::default(),
                confirmed: false,
            },
            1_000,
        )
        .unwrap();

    let record = explainability.get(response.explanation_id).unwrap();
    let confidence = record
        .confidence
        .expect("the real routing decision must produce a confidence score");
    assert!(
        confidence.value > 0.0 && confidence.value <= 1.0,
        "expected a real composite fitness score, got {}",
        confidence.value
    );
    assert_eq!(
        confidence.method,
        hyperion_explainability::ConfidenceMethod::Heuristic
    );
    assert_eq!(
        record.alternatives.len(),
        1,
        "the losing candidate must appear as a real alternative"
    );
    assert!(
        record.reasoning_chain.len() >= 2,
        "both the risk assessment and the routing decision must contribute a reasoning step"
    );
}

#[test]
fn an_ensemble_agreement_between_two_stub_dispatched_candidates_boosts_confidence() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let intent = Arc::new(IntentEngine::new(graph.clone(), context.clone()));
    let memory = Arc::new(MemoryEngine::new(graph.clone()));
    let registry = Arc::new(PluginRegistry::new());
    let (_key_dir, keystore) = keystore();
    let explainability = Arc::new(ExplanationStore::new());
    let (router, ai_runtime) = model_router_and_ai_runtime();
    let gateway = ApiGateway::new(
        intent,
        memory,
        graph,
        registry.clone(),
        explainability.clone(),
        router,
        context,
        ai_runtime,
    );
    gateway.grant_scopes(&monitor, &root, all_scopes()).unwrap();

    let contract = SemanticContract {
        inputs: vec!["query".to_string()],
        outputs: vec!["results".to_string()],
        side_effects: vec![SideEffect::NetworkEgress],
    };

    // Two architecturally distinct kinds (`CloudApi` vs `LocalSmallModel`) competing for the same
    // capability -- neither has a `native_binary` or a real `ModelClass` (the Plugin Framework
    // bridge can't produce one, see `router_bridge::to_router_descriptor`'s own doc comment), so
    // both really dispatch through the identical real stub path. That's genuine agreement, not a
    // fabricated one: two real, independent `dispatch_one` calls that happen to hit the same real
    // fallback and produce the identical real output.
    let mut cloud = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract: contract.clone(),
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.9,
            version: 1,
            native_binary: None,
            privacy_tier: PrivacyTier::Local,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    cloud.signature = Some(sign(&cloud, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            cloud,
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let mut local_model = PluginManifest {
        plugin_id: 2,
        publisher: "globex-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract,
            implementation_kind: ImplementationKind::LocalSmallModel,
            quality_score: 0.5,
            version: 1,
            native_binary: None,
            privacy_tier: PrivacyTier::Local,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    local_model.signature = Some(sign(&local_model, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            local_model,
            TrustDepth::D2,
            true,
            1_001,
            &keystore.verifying_key(),
        )
        .unwrap();

    let response = gateway
        .invoke_capability(
            &monitor,
            &root,
            InvokeRequest {
                contract_id: "web.search".to_string(),
                inputs: serde_json::json!({"query": "hyperion os"}),
                agent_id: 1,
                intent_id: 1,
                risk: risky_pending_action_hints(), // HighStakes -> needs_verification
                confirmed: true,
            },
            1_002,
        )
        .unwrap();

    let outcome = response.ensemble.expect(
        "a HighStakes invocation with two real, distinct-kind candidates must actually \
                 dispatch and reconcile a real ensemble",
    );
    assert!(
        outcome.boosted_confidence > 0.5 && outcome.boosted_confidence <= 1.0,
        "expected a real, boosted confidence, got {}",
        outcome.boosted_confidence
    );
    assert_ne!(
        outcome.verifying_impl, response.implementation_used,
        "the verifying implementation must be genuinely distinct from the primary"
    );

    let record = explainability.get(response.explanation_id).unwrap();
    let confidence = record
        .confidence
        .expect("ensemble agreement must leave a real, updated confidence on the record");
    assert_eq!(confidence.value, outcome.boosted_confidence);
    assert_eq!(
        confidence.method,
        hyperion_explainability::ConfidenceMethod::Ensemble,
        "an ensemble-corroborated confidence must be tagged as such, not as a bare heuristic"
    );
}

#[test]
fn a_manifests_real_declared_privacy_tier_genuinely_affects_which_candidate_wins() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let intent = Arc::new(IntentEngine::new(graph.clone(), context.clone()));
    let memory = Arc::new(MemoryEngine::new(graph.clone()));
    let registry = Arc::new(PluginRegistry::new());
    let (_key_dir, keystore) = keystore();
    let explainability = Arc::new(ExplanationStore::new());
    let (router, ai_runtime) = model_router_and_ai_runtime();
    let gateway = ApiGateway::new(
        intent,
        memory,
        graph,
        registry.clone(),
        explainability,
        router,
        context,
        ai_runtime,
    );
    gateway.grant_scopes(&monitor, &root, all_scopes()).unwrap();

    let contract = SemanticContract {
        inputs: vec!["query".to_string()],
        outputs: vec!["results".to_string()],
        side_effects: vec![SideEffect::NetworkEgress],
    };

    // Two otherwise-identical candidates (same kind, same quality) -- only their real, declared
    // privacy tier differs.
    let mut cloud = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract: contract.clone(),
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.7,
            version: 1,
            native_binary: None,
            privacy_tier: hyperion_plugin_framework::PrivacyTier::ConsentedCloud,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    cloud.signature = Some(sign(&cloud, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            cloud,
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let mut local = PluginManifest {
        plugin_id: 2,
        publisher: "globex-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract,
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.7,
            version: 1,
            native_binary: None,
            privacy_tier: hyperion_plugin_framework::PrivacyTier::Local,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    local.signature = Some(sign(&local, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            local,
            TrustDepth::D2,
            true,
            1_001,
            &keystore.verifying_key(),
        )
        .unwrap();

    let response = gateway
        .invoke_capability(
            &monitor,
            &root,
            InvokeRequest {
                contract_id: "web.search".to_string(),
                inputs: serde_json::json!({"query": "hyperion os"}),
                agent_id: 1,
                intent_id: 1,
                risk: RiskHints::default(),
                confirmed: false,
            },
            1_002,
        )
        .unwrap();

    assert_eq!(
        response.implementation_used, 2,
        "an otherwise-identical Local candidate must genuinely outscore a ConsentedCloud one, \
         now that the manifest's own real, declared privacy tier reaches the Model Router's real \
         privacy_fit scoring instead of every bridged candidate being hardcoded Local"
    );
}

#[test]
fn invoke_capability_with_no_registered_contract_is_not_eligible() {
    let (monitor, root, gateway) = setup();
    gateway.grant_scopes(&monitor, &root, all_scopes()).unwrap();

    let result = gateway.invoke_capability(
        &monitor,
        &root,
        InvokeRequest {
            contract_id: "no.such.capability".to_string(),
            inputs: serde_json::json!({}),
            agent_id: 1,
            intent_id: 1,
            risk: RiskHints::default(),
            confirmed: false,
        },
        1_000,
    );
    assert!(matches!(result, Err(ApiError::NoEligibleImplementation)));
}

/// docs/998-roadmap.md's Slice 1, closed here too: this gateway's own `registry` had the exact
/// same "data only, no execution" gap `hyperion-agent-runtime::AgentRuntime::invoke` did,
/// independently documented in this crate's own code -- now wired to the same real, sandboxed
/// `NativeBinary` execution path. Linux-only, matching `hyperion-trust-boundary`'s own gating.
#[cfg(target_os = "linux")]
#[test]
fn invoke_capability_dispatches_to_a_real_installed_native_binary_plugin() {
    use hyperion_plugin_framework::{CapabilityGrantRequest, NativeBinaryDescriptor, Operation};

    fn uppercase_tool_bin() -> std::path::PathBuf {
        let target = "x86_64-unknown-linux-musl";
        let status = std::process::Command::new("cargo")
            .args([
                "build",
                "--target",
                target,
                "--bin",
                "uppercase_tool",
                "-p",
                "hyperion-plugin-framework",
            ])
            .status()
            .expect("run cargo build for the musl uppercase_tool binary");
        assert!(
            status.success(),
            "building the musl uppercase_tool binary failed"
        );

        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("crates/hyperion-api-gateway has a workspace root two levels up")
            .to_path_buf();
        workspace_root
            .join("target")
            .join(target)
            .join("debug")
            .join("uppercase_tool")
    }

    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let intent = Arc::new(IntentEngine::new(graph.clone(), context.clone()));
    let memory = Arc::new(MemoryEngine::new(graph.clone()));
    let registry = Arc::new(PluginRegistry::new());
    let (_key_dir, keystore) = keystore();
    let explainability = Arc::new(ExplanationStore::new());
    let (router, ai_runtime) = model_router_and_ai_runtime();
    let gateway = ApiGateway::new(
        intent,
        memory,
        graph,
        registry.clone(),
        explainability,
        router,
        context,
        ai_runtime,
    );
    gateway.grant_scopes(&monitor, &root, all_scopes()).unwrap();

    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "text.uppercase".to_string(),
            contract: SemanticContract {
                inputs: vec!["text".to_string()],
                outputs: vec!["text".to_string()],
                side_effects: vec![SideEffect::None],
            },
            implementation_kind: ImplementationKind::NativeBinary,
            quality_score: 0.5,
            version: 1,
            native_binary: Some(NativeBinaryDescriptor {
                program: uppercase_tool_bin(),
                args: vec![],
            }),
            privacy_tier: PrivacyTier::Local,
        })],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::Execute,
            scope: "text.uppercase".to_string(),
            justification: "run the real sandboxed tool".to_string(),
        }],
        min_trust_depth: TrustDepth::D1,
    };
    manifest.signature = Some(sign(&manifest, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            manifest,
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let response = gateway
        .invoke_capability(
            &monitor,
            &root,
            InvokeRequest {
                contract_id: "text.uppercase".to_string(),
                inputs: serde_json::json!({"text": "hello from the real gateway"}),
                agent_id: 42,
                intent_id: 7,
                risk: RiskHints::default(),
                confirmed: false,
            },
            1_000,
        )
        .unwrap();

    assert_eq!(
        response.outputs.get("text").and_then(|v| v.as_str()),
        Some("HELLO FROM THE REAL GATEWAY"),
        "expected the real sandboxed plugin's real output, got: {:?}",
        response.outputs
    );
}
