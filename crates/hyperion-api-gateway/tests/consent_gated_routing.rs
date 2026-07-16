//! This crate's own named "`cloud_consent` feeding the Model Router" gap, real for the first
//! time (see the crate doc comment): `ApiGateway::new_with_consent_ledger` wires a real
//! `hyperion-privacy::ConsentLedger` so `invoke_capability`'s own `cloud_consent` input becomes a
//! real, never-assumed lookup instead of the previous permissive `true` default.

use std::sync::Arc;

use hyperion_ai_runtime::{LocalAiRuntime, MockBackend};
use hyperion_api_gateway::{ApiError, ApiGateway, ApiScope, InvokeRequest, RiskHints};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
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
use hyperion_privacy::{ConsentLedger, DataScope};

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

fn model_router_and_ai_runtime() -> (Arc<ModelRouter>, Arc<LocalAiRuntime>) {
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 4_096));
    (Arc::new(ModelRouter::new(ai_runtime.clone())), ai_runtime)
}

/// A single, real installed plugin whose only implementation declares
/// `PrivacyTier::ConsentedCloud` -- the one candidate the Model Router's own real privacy gate
/// excludes without a standing consent grant.
fn install_consented_cloud_only_plugin(
    monitor: &mut CapabilityMonitor,
    root: &hyperion_capability::CapabilityToken,
    registry: &PluginRegistry,
    keystore: &Keystore,
) {
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
            privacy_tier: PrivacyTier::ConsentedCloud,
            resource_profile: None,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = Some(sign(&manifest, keystore));
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
    root: &hyperion_capability::CapabilityToken,
) -> Result<(), ApiError> {
    gateway
        .invoke_capability(
            monitor,
            root,
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
        .map(|_| ())
}

#[test]
fn without_a_consent_ledger_wired_a_consented_cloud_candidate_is_still_selected_by_default() {
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
    gateway
        .grant_scopes(
            &monitor,
            &root,
            [ApiScope::CapabilityInvoke].into_iter().collect(),
        )
        .unwrap();
    install_consented_cloud_only_plugin(&mut monitor, &root, &registry, &keystore);

    invoke(&gateway, &monitor, &root).expect(
        "with no ConsentLedger wired, cloud_consent must stay the permissive true default, \
         exactly as it behaved before this gap was closed",
    );
}

#[test]
fn with_a_consent_ledger_wired_and_no_standing_grant_a_consented_cloud_only_candidate_is_excluded()
{
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
    let consent_ledger = Arc::new(ConsentLedger::new());
    let gateway = ApiGateway::new_with_consent_ledger(
        intent,
        memory,
        graph,
        registry.clone(),
        explainability,
        router,
        context,
        ai_runtime,
        consent_ledger,
    );
    gateway
        .grant_scopes(
            &monitor,
            &root,
            [ApiScope::CapabilityInvoke].into_iter().collect(),
        )
        .unwrap();
    install_consented_cloud_only_plugin(&mut monitor, &root, &registry, &keystore);

    let result = invoke(&gateway, &monitor, &root);
    assert!(
        matches!(result, Err(ApiError::NoEligibleImplementation)),
        "a real ConsentLedger with no standing grant must never assume consent -- the only \
         registered candidate is ConsentedCloud-tier, so it must be excluded and nothing else \
         is eligible; got: {result:?}"
    );
}

#[test]
fn after_a_real_consent_grant_the_consented_cloud_candidate_becomes_eligible() {
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
    let consent_ledger = Arc::new(ConsentLedger::new());
    let gateway = ApiGateway::new_with_consent_ledger(
        intent,
        memory,
        graph,
        registry.clone(),
        explainability,
        router,
        context,
        ai_runtime,
        consent_ledger.clone(),
    );
    gateway
        .grant_scopes(
            &monitor,
            &root,
            [ApiScope::CapabilityInvoke].into_iter().collect(),
        )
        .unwrap();
    install_consented_cloud_only_plugin(&mut monitor, &root, &registry, &keystore);

    // Still excluded before any grant exists.
    assert!(matches!(
        invoke(&gateway, &monitor, &root),
        Err(ApiError::NoEligibleImplementation)
    ));

    consent_ledger
        .request(
            &monitor,
            &root,
            root.token_id().0,
            DataScope::Capability("web.search".to_string()),
            "user asked to search the web",
            None,
            1_000,
        )
        .unwrap();

    invoke(&gateway, &monitor, &root).expect(
        "a real standing consent grant scoped to this exact capability must make the \
         ConsentedCloud candidate eligible",
    );
}
