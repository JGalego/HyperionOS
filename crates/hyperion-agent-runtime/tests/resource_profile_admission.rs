//! `Implementation.resource_profile`'s real consumer: `AgentRuntime::prepare_invoke` now submits
//! a capability's own publisher-declared `ResourceVector` to the Scheduler instead of the same
//! fixed, one-token-per-second stand-in for every capability. A capability that declares a
//! reservation larger than the runtime's own headroom must be genuinely denied by the real
//! admission algorithm -- something the old hardcoded-minimal request could never trigger,
//! regardless of what a capability actually needed.

use std::sync::Arc;

use hyperion_agent_runtime::{AgentManifest, AgentRuntime, InvokeOutcome, TrustTier};
use hyperion_ai_runtime::{LocalAiRuntime, MockBackend};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{
    sign, CapabilityGrantRequest, CapabilityManifest, Contribution, ImplementationKind, Operation,
    PluginManifest, PluginRegistry, PrivacyTier, SemanticContract, SideEffect, TrustDepth,
};
use hyperion_scheduler::ResourceVector;
use serde_json::json;

fn install_manifest(
    registry: &PluginRegistry,
    monitor: &mut CapabilityMonitor,
    root: &hyperion_capability::CapabilityToken,
    keystore: &Keystore,
    capability_id: &str,
    resource_profile: Option<ResourceVector>,
) {
    let mut plugin_manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: capability_id.to_string(),
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
            resource_profile,
        })],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::Execute,
            scope: capability_id.to_string(),
            justification: "declare a real resource profile".to_string(),
        }],
        min_trust_depth: TrustDepth::D1,
    };
    plugin_manifest.signature = Some(sign(&plugin_manifest, keystore));
    registry
        .install(
            monitor,
            root,
            plugin_manifest,
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();
}

fn spawn_instance(
    runtime: &AgentRuntime,
    monitor: &CapabilityMonitor,
    root: &hyperion_capability::CapabilityToken,
    capability_id: &str,
) -> u64 {
    let agent_manifest = AgentManifest {
        specialization: "tool-user".to_string(),
        baseline_capabilities: vec![capability_id.to_string()],
        requestable_capabilities: vec![],
        trust_tier: TrustTier::System,
    };
    runtime
        .spawn(monitor, root, agent_manifest, Some(1))
        .unwrap()
}

#[test]
fn a_capability_with_a_declared_reservation_over_headroom_is_denied_by_the_real_scheduler() {
    let scratch = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&scratch.path().join("device.key")).unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();

    // The runtime's own single ledger dimension has a capacity of 100 (`DEFAULT_QUOTA`) --
    // declaring far more than that must be genuinely refused, not silently admitted.
    install_manifest(
        &registry,
        &mut monitor,
        &root,
        &keystore,
        "heavy.task",
        Some(ResourceVector {
            inference_tokens_per_sec: 1_000,
            ..Default::default()
        }),
    );

    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));
    let runtime =
        AgentRuntime::new_with_netstack_and_plugins(ai_runtime, None, Some(Arc::new(registry)));
    let instance_id = spawn_instance(&runtime, &monitor, &root, "heavy.task");

    let outcome = runtime
        .invoke(&monitor, &root, instance_id, "heavy.task", json!({}))
        .unwrap();

    assert!(
        matches!(outcome, InvokeOutcome::QuotaExceeded),
        "a declared reservation of 1000 tokens/sec against a 100-token ledger must be denied by \
         the real Scheduler, got: {outcome:?}"
    );
}

#[test]
fn a_capability_with_no_declared_profile_still_uses_the_fixed_default_request() {
    let scratch = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&scratch.path().join("device.key")).unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();

    install_manifest(
        &registry,
        &mut monitor,
        &root,
        &keystore,
        "light.task",
        None,
    );

    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));
    let runtime =
        AgentRuntime::new_with_netstack_and_plugins(ai_runtime, None, Some(Arc::new(registry)));
    let instance_id = spawn_instance(&runtime, &monitor, &root, "light.task");

    let outcome = runtime
        .invoke(&monitor, &root, instance_id, "light.task", json!({}))
        .unwrap();

    assert!(
        !matches!(outcome, InvokeOutcome::QuotaExceeded),
        "with no declared resource_profile the fixed, minimal default request must still be \
         admitted against a 100-token ledger, got: {outcome:?}"
    );
}
