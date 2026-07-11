//! Mirrors every other crate in this workspace: every scoped route is
//! gated, re-checked live against the monitor.

use std::collections::HashSet;
use std::sync::Arc;

use hyperion_ai_runtime::{LocalAiRuntime, MockBackend};
use hyperion_api_gateway::{ApiError, ApiGateway, ApiScope};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_explainability::ExplanationStore;
use hyperion_intent::IntentEngine;
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_memory::MemoryEngine;
use hyperion_model_router::ModelRouter;
use hyperion_plugin_framework::PluginRegistry;

/// Returns the same `Arc<LocalAiRuntime>` `ModelRouter` was built with -- see
/// `ApiGateway::new`'s own doc comment on why a second, disconnected instance would be wrong.
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

#[test]
fn a_route_with_no_scope_grant_at_all_is_denied() {
    let (monitor, root, gateway) = setup();
    let result = gateway.kg_write(&monitor, &root, "Note", serde_json::json!({}));
    assert!(matches!(
        result,
        Err(ApiError::InsufficientScope(ApiScope::KgWrite))
    ));
}

#[test]
fn a_route_with_a_different_scope_granted_is_still_denied() {
    let (monitor, root, gateway) = setup();
    gateway
        .grant_scopes(
            &monitor,
            &root,
            [ApiScope::KgQuery].into_iter().collect::<HashSet<_>>(),
        )
        .unwrap();

    let result = gateway.kg_write(&monitor, &root, "Note", serde_json::json!({}));
    assert!(matches!(
        result,
        Err(ApiError::InsufficientScope(ApiScope::KgWrite))
    ));
}

#[test]
fn revoking_the_token_blocks_further_access_re_checked_live() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let delegate = monitor
        .cap_derive(&root, RightsMask::all(), None, TrustBoundaryId(2))
        .unwrap();
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

    gateway
        .grant_scopes(
            &monitor,
            &delegate,
            [ApiScope::KgWrite].into_iter().collect::<HashSet<_>>(),
        )
        .unwrap();
    assert!(gateway
        .kg_write(&monitor, &delegate, "Note", serde_json::json!({}))
        .is_ok());

    monitor.cap_revoke(&delegate);

    assert!(matches!(
        gateway.kg_write(&monitor, &delegate, "Note", serde_json::json!({})),
        Err(ApiError::Unauthorized)
    ));
}
