//! `hyperion-scheduler`'s own named "model-tier degradation" gap, proven at this crate's one real
//! production caller: `AgentRuntime::prepare_invoke` now wires a real `ModelRouter` into its own
//! `Scheduler`, and names the invoked capability on every submitted `TaskDescriptor` -- a
//! capability whose own declared request doesn't fit the ledger is admitted at a cheaper,
//! separately-registered Model Router implementation instead of being refused outright.

use std::collections::HashMap;
use std::sync::Arc;

use hyperion_agent_runtime::{AgentManifest, AgentRuntime, InvokeOutcome, TrustTier};
use hyperion_ai_runtime::{LocalAiRuntime, MockBackend};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_model_router::{
    CostModel, ImplId, ImplKind, ImplementationDescriptor, ModelRouter, PrivacyTier as RouterTier,
    ResourceCost, RolloutStage,
};
use hyperion_plugin_framework::{
    sign, CapabilityGrantRequest, CapabilityManifest, Contribution, ImplementationKind, Operation,
    PluginManifest, PluginRegistry, PrivacyTier, SemanticContract, SideEffect, TrustDepth,
};
use hyperion_scheduler::ResourceVector;
use serde_json::json;

#[test]
fn a_capability_whose_own_declared_request_does_not_fit_is_admitted_via_a_cheaper_model_router_implementation(
) {
    let scratch = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&scratch.path().join("device.key")).unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);

    // The Plugin Framework declares a resource_profile far larger than the runtime's own
    // 100-token ledger -- the capability's own request alone must not fit.
    let registry = PluginRegistry::new();
    let mut plugin_manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "heavy.task".to_string(),
            contract: SemanticContract {
                inputs: vec!["text".to_string()],
                outputs: vec!["text".to_string()],
                side_effects: vec![SideEffect::None],
            },
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.5,
            version: 1,
            native_binary: None,
            privacy_tier: PrivacyTier::Local,
            resource_profile: Some(ResourceVector {
                inference_tokens_per_sec: 1_000,
                ..Default::default()
            }),
        })],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::Execute,
            scope: "heavy.task".to_string(),
            justification: "declare a request too large to fit alone".to_string(),
        }],
        min_trust_depth: TrustDepth::D1,
    };
    plugin_manifest.signature = Some(sign(&plugin_manifest, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            plugin_manifest,
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    // A separate, real Model Router registration for the *same* capability declares a cheap
    // resource cost that genuinely fits.
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));
    let model_router = Arc::new(ModelRouter::new(ai_runtime.clone()));
    model_router
        .register_implementation(
            &monitor,
            &root,
            ImplementationDescriptor {
                impl_id: ImplId(1),
                capability_id: "heavy.task".to_string(),
                kind: ImplKind::CloudApi,
                model_class: None,
                privacy_tier: RouterTier::Local,
                cost_model: CostModel::Free,
                quality_profile: HashMap::new(),
                declared_latency_ms: 100,
                rollout_stage: RolloutStage::Shadow,
                resource_cost: Some(ResourceCost {
                    inference_tokens_per_sec: 10,
                    ..Default::default()
                }),
            },
        )
        .unwrap();
    model_router
        .set_rollout_stage(&monitor, &root, ImplId(1), RolloutStage::Ga)
        .unwrap();

    let runtime = AgentRuntime::new_with_netstack_and_plugins_and_memory_and_model_router(
        ai_runtime,
        None,
        Some(Arc::new(registry)),
        None,
        Some(model_router),
    );

    let agent_manifest = AgentManifest {
        specialization: "tool-user".to_string(),
        baseline_capabilities: vec!["heavy.task".to_string()],
        requestable_capabilities: vec![],
        trust_tier: TrustTier::System,
    };
    let instance_id = runtime
        .spawn(&monitor, &root, agent_manifest, Some(1))
        .unwrap();

    let outcome = runtime
        .invoke(&monitor, &root, instance_id, "heavy.task", json!({}))
        .unwrap();

    assert!(
        matches!(outcome, InvokeOutcome::Result(_)),
        "a capability whose own declared request doesn't fit must still be admitted via a \
         cheaper, real Model Router implementation, got: {outcome:?}"
    );
}
