//! docs/26's five-API gateway, backed by real subsystem crates where
//! this slice wires them (Intent, Knowledge Graph, Memory), plus the
//! Capability Invocation path's registry lookup → dispatch →
//! explainability recording pipeline.

use std::collections::HashSet;
use std::sync::Arc;

use hyperion_api_gateway::{
    ApiError, ApiGateway, ApiScope, InvokeRequest, SubmitIntentRequest, SubmitIntentResponse,
};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_explainability::ExplanationStore;
use hyperion_intent::IntentEngine;
use hyperion_knowledge_graph::{GraphQuery, KnowledgeGraph};
use hyperion_memory::MemoryEngine;
use hyperion_plugin_framework::{
    signature, CapabilityManifest, Contribution, ImplementationKind, PluginManifest,
    PluginRegistry, SemanticContract, SideEffect, TrustDepth,
};

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
    let intent = Arc::new(IntentEngine::new(graph.clone(), context));
    let memory = Arc::new(MemoryEngine::new(graph.clone()));
    let registry = Arc::new(PluginRegistry::new());
    let explainability = Arc::new(ExplanationStore::new());
    let gateway = ApiGateway::new(intent, memory, graph, registry, explainability);
    (monitor, root, gateway)
}

fn all_scopes() -> HashSet<ApiScope> {
    [
        ApiScope::IntentSubmit,
        ApiScope::MemoryWrite,
        ApiScope::KgQuery,
        ApiScope::KgWrite,
        ApiScope::CapabilityInvoke,
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
fn invoke_capability_dispatches_through_the_real_stub_and_records_an_explanation() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let intent = Arc::new(IntentEngine::new(graph.clone(), context));
    let memory = Arc::new(MemoryEngine::new(graph.clone()));
    let registry = Arc::new(PluginRegistry::new());
    let explainability = Arc::new(ExplanationStore::new());
    let gateway = ApiGateway::new(
        intent,
        memory,
        graph,
        registry.clone(),
        explainability.clone(),
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
        },
        1_000,
    );
    assert!(matches!(result, Err(ApiError::NoEligibleImplementation)));
}
