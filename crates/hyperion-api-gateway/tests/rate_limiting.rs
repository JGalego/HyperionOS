//! docs/26's own named "Rate/quota enforcement... no algorithm given" gap, closed for
//! `ApiGateway::invoke_capability`: a real, per-caller fixed-window counter
//! (`RateLimitPolicy`/`ApiGateway::check_rate_limit`), the same algorithm
//! `hyperion-netstack`'s own `DomainEgressGrant` rate limiting already established.

use std::collections::HashSet;
use std::sync::Arc;

use hyperion_ai_runtime::{LocalAiRuntime, MockBackend};
use hyperion_api_gateway::{
    ApiError, ApiGateway, ApiScope, InvokeRequest, RateLimitPolicy, RiskHints,
};
use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_crypto::Keystore;
use hyperion_explainability::ExplanationStore;
use hyperion_intent::IntentEngine;
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_memory::MemoryEngine;
use hyperion_model_router::ModelRouter;
use hyperion_plugin_framework::{
    sign, CapabilityManifest, Contribution, ImplementationKind, PluginManifest, PluginRegistry,
    PrivacyTier, SemanticContract, SideEffect, TrustDepth,
};

fn all_scopes() -> HashSet<ApiScope> {
    [ApiScope::CapabilityInvoke].into_iter().collect()
}

fn setup() -> (CapabilityMonitor, CapabilityToken, ApiGateway) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let intent = Arc::new(IntentEngine::new(graph.clone(), context.clone()));
    let memory = Arc::new(MemoryEngine::new(graph.clone()));
    let registry = Arc::new(PluginRegistry::new());
    let explainability = Arc::new(ExplanationStore::new());
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 4_096));
    let router = Arc::new(ModelRouter::new(ai_runtime.clone()));
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
    install_web_search_plugin(&mut monitor, &root, &registry);
    (monitor, root, gateway)
}

fn install_web_search_plugin(
    monitor: &mut CapabilityMonitor,
    root: &CapabilityToken,
    registry: &PluginRegistry,
) {
    let key_dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&key_dir.path().join("device.key")).unwrap();
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
            resource_profile: None,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = Some(sign(&manifest, &keystore));
    registry
        .install(
            monitor,
            root,
            manifest,
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();
}

fn invoke(
    gateway: &ApiGateway,
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    now: u64,
) -> Result<(), ApiError> {
    gateway
        .invoke_capability(
            monitor,
            token,
            InvokeRequest {
                contract_id: "web.search".to_string(),
                inputs: serde_json::json!({"query": "hyperion os"}),
                agent_id: 42,
                intent_id: 7,
                risk: RiskHints::default(),
                confirmed: false,
            },
            now,
        )
        .map(|_| ())
}

#[test]
fn calls_within_the_default_window_limit_all_succeed() {
    let (monitor, root, gateway) = setup();
    for _ in 0..5 {
        invoke(&gateway, &monitor, &root, 1_000).unwrap();
    }
}

#[test]
fn a_lowered_override_rejects_once_its_own_smaller_window_is_exhausted() {
    let (monitor, root, gateway) = setup();
    gateway
        .set_rate_limit(
            &monitor,
            &root,
            RateLimitPolicy {
                calls_per_window: 2,
                window_secs: 60,
            },
            1_000,
        )
        .unwrap();

    invoke(&gateway, &monitor, &root, 1_000).unwrap();
    invoke(&gateway, &monitor, &root, 1_010).unwrap();
    assert!(matches!(
        invoke(&gateway, &monitor, &root, 1_020),
        Err(ApiError::RateLimited)
    ));
}

#[test]
fn the_window_resets_once_window_secs_has_really_elapsed() {
    let (monitor, root, gateway) = setup();
    gateway
        .set_rate_limit(
            &monitor,
            &root,
            RateLimitPolicy {
                calls_per_window: 1,
                window_secs: 60,
            },
            1_000,
        )
        .unwrap();

    invoke(&gateway, &monitor, &root, 1_000).unwrap();
    assert!(matches!(
        invoke(&gateway, &monitor, &root, 1_030),
        Err(ApiError::RateLimited)
    ));
    // A real 60-second window has now really elapsed since the first call.
    invoke(&gateway, &monitor, &root, 1_061).unwrap();
}

#[test]
fn rate_limiting_is_scoped_per_token_not_shared_globally() {
    let (mut monitor, root, gateway) = setup();
    let other = monitor
        .cap_derive(&root, RightsMask::all(), None, TrustBoundaryId(2))
        .unwrap();
    gateway
        .grant_scopes(&monitor, &other, all_scopes())
        .unwrap();
    gateway
        .set_rate_limit(
            &monitor,
            &root,
            RateLimitPolicy {
                calls_per_window: 1,
                window_secs: 60,
            },
            1_000,
        )
        .unwrap();

    invoke(&gateway, &monitor, &root, 1_000).unwrap();
    assert!(matches!(
        invoke(&gateway, &monitor, &root, 1_010),
        Err(ApiError::RateLimited)
    ));
    // A completely different token, never given its own override, must still get the real
    // (generous) default budget -- one token's exhausted quota never charges another's.
    invoke(&gateway, &monitor, &other, 1_010).unwrap();
}

#[test]
fn set_rate_limit_requires_a_live_token() {
    let (mut monitor, root, gateway) = setup();
    monitor.cap_revoke(&root);
    assert!(matches!(
        gateway.set_rate_limit(
            &monitor,
            &root,
            RateLimitPolicy {
                calls_per_window: 1,
                window_secs: 60,
            },
            1_000,
        ),
        Err(ApiError::Unauthorized)
    ));
}
