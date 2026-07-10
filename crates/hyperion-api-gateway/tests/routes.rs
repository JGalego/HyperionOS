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
use hyperion_explainability::ExplanationStore;
use hyperion_intent::IntentEngine;
use hyperion_knowledge_graph::{GraphQuery, KnowledgeGraph};
use hyperion_memory::MemoryEngine;
use hyperion_model_router::ModelRouter;
use hyperion_plugin_framework::{
    signature, CapabilityManifest, Contribution, ImplementationKind, PluginManifest,
    PluginRegistry, SemanticContract, SideEffect, TrustDepth,
};

fn model_router() -> Arc<ModelRouter> {
    Arc::new(ModelRouter::new(Arc::new(LocalAiRuntime::new(
        Box::new(MockBackend),
        4_096,
    ))))
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
    let gateway = ApiGateway::new(
        intent,
        memory,
        graph,
        registry,
        explainability,
        model_router(),
        context,
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
    let explainability = Arc::new(ExplanationStore::new());
    let gateway = ApiGateway::new(
        intent,
        memory,
        graph,
        registry.clone(),
        explainability.clone(),
        model_router(),
        context,
    );
    gateway.grant_scopes(&monitor, &root, all_scopes()).unwrap();

    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: 0,
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
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = signature(&manifest);
    registry
        .install(&mut monitor, &root, manifest, TrustDepth::D2, true, 1_000)
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
    let explainability = Arc::new(ExplanationStore::new());
    let router = model_router();
    let gateway = ApiGateway::new(
        intent,
        memory,
        graph,
        registry.clone(),
        explainability,
        router.clone(),
        context,
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
        signature: 0,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract: contract.clone(),
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.7,
            version: 1,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    circuit_open_plugin.signature = signature(&circuit_open_plugin);
    registry
        .install(
            &mut monitor,
            &root,
            circuit_open_plugin,
            TrustDepth::D2,
            true,
            1_000,
        )
        .unwrap();

    // Plugin 2: slightly lower quality, but healthy.
    let mut healthy_plugin = PluginManifest {
        plugin_id: 2,
        publisher: "globex-plugins".to_string(),
        signature: 0,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract,
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.6,
            version: 1,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    healthy_plugin.signature = signature(&healthy_plugin);
    registry
        .install(
            &mut monitor,
            &root,
            healthy_plugin,
            TrustDepth::D2,
            true,
            1_001,
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
    let explainability = Arc::new(ExplanationStore::new());
    let gateway = ApiGateway::new(
        intent,
        memory,
        graph,
        registry.clone(),
        explainability,
        model_router(),
        context,
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
        signature: 0,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract: contract.clone(),
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.3,
            version: 1,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    low.signature = signature(&low);
    registry
        .install(&mut monitor, &root, low, TrustDepth::D2, true, 1_000)
        .unwrap();

    let mut high = PluginManifest {
        plugin_id: 2,
        publisher: "globex-plugins".to_string(),
        signature: 0,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract,
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.9,
            version: 1,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    high.signature = signature(&high);
    registry
        .install(&mut monitor, &root, high, TrustDepth::D2, true, 1_001)
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
    let explainability = Arc::new(ExplanationStore::new());
    let gateway = ApiGateway::new(
        intent,
        memory,
        graph,
        registry.clone(),
        explainability,
        model_router(),
        context,
    );
    gateway.grant_scopes(&monitor, &root, all_scopes()).unwrap();

    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: 0,
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
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = signature(&manifest);
    registry
        .install(&mut monitor, &root, manifest, TrustDepth::D2, true, 1_000)
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
    let explainability = Arc::new(ExplanationStore::new());
    let gateway = ApiGateway::new(
        intent,
        memory,
        graph,
        registry.clone(),
        explainability.clone(),
        model_router(),
        context,
    );
    gateway.grant_scopes(&monitor, &root, all_scopes()).unwrap();

    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: 0,
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
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = signature(&manifest);
    registry
        .install(&mut monitor, &root, manifest, TrustDepth::D2, true, 1_000)
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
    let explainability = Arc::new(ExplanationStore::new());
    let gateway = ApiGateway::new(
        intent,
        memory,
        graph,
        registry.clone(),
        explainability.clone(),
        model_router(),
        context,
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
        signature: 0,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract: contract.clone(),
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.3,
            version: 1,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    low.signature = signature(&low);
    registry
        .install(&mut monitor, &root, low, TrustDepth::D2, true, 1_000)
        .unwrap();

    let mut high = PluginManifest {
        plugin_id: 2,
        publisher: "globex-plugins".to_string(),
        signature: 0,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "web.search".to_string(),
            contract,
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.9,
            version: 1,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    high.signature = signature(&high);
    registry
        .install(&mut monitor, &root, high, TrustDepth::D2, true, 1_001)
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
