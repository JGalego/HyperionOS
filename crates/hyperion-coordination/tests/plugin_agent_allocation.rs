//! docs/998-roadmap.md's Resourceful pillar: a plugin-contributed `Contribution::Agent` really
//! competes for task allocation, through the exact same
//! `hyperion_coordination::best_fit_manifest_with_plugins` call
//! `CoordinationSession::allocate` itself makes (via `AgentRuntime::plugin_registry`) -- not a
//! parallel, test-only path.

use std::sync::Arc;

use hyperion_agent_runtime::{AgentRuntime, TrustTier};
use hyperion_ai_runtime::{LocalAiRuntime, MockBackend};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_coordination::best_fit_manifest_with_plugins;
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{
    sign, AgentContribution, CapabilityGrantRequest, Contribution, Operation, PluginManifest,
    PluginRegistry, TrustDepth,
};

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

fn install_translator_agent(registry: &PluginRegistry) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let (_dir, keystore) = keystore();

    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Agent(AgentContribution {
            specialization: "custom-translator".to_string(),
            baseline_capabilities: vec!["unknown.translate_menu".to_string()],
            requestable_capabilities: vec![],
        })],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::Execute,
            scope: "custom-translator".to_string(),
            justification: "the agent must be dispatchable".to_string(),
        }],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = Some(sign(&manifest, &keystore));

    registry
        .install(
            &mut monitor,
            &root,
            manifest,
            TrustDepth::D0,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();
}

#[test]
fn no_built_in_specialization_covers_a_capability_nothing_declares() {
    let required = vec!["unknown.translate_menu".to_string()];
    assert!(best_fit_manifest_with_plugins(&required, None).is_none());
}

#[test]
fn a_plugin_contributed_agent_is_selected_when_no_built_in_specialization_fits() {
    let registry = PluginRegistry::new();
    install_translator_agent(&registry);

    let required = vec!["unknown.translate_menu".to_string()];
    let manifest = best_fit_manifest_with_plugins(&required, Some(&registry))
        .expect("the plugin-contributed agent must be found once the registry is consulted");

    assert_eq!(manifest.specialization, "custom-translator");
    assert_eq!(manifest.trust_tier, TrustTier::Community);
}

#[test]
fn agent_runtime_exposes_its_plugin_registry_and_coordination_finds_agents_through_it() {
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));
    let registry = Arc::new(PluginRegistry::new());
    install_translator_agent(&registry);

    let agent_runtime =
        AgentRuntime::new_with_netstack_and_plugins(ai_runtime, None, Some(registry));

    // The exact accessor `CoordinationSession::allocate`'s "no existing candidate" branch calls.
    let live_registry = agent_runtime.plugin_registry().map(std::sync::Arc::as_ref);
    let required = vec!["unknown.translate_menu".to_string()];
    let manifest = best_fit_manifest_with_plugins(&required, live_registry)
        .expect("AgentRuntime::plugin_registry must expose the real, installed registry");

    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let instance_id = agent_runtime
        .spawn(&monitor, &token, manifest, None)
        .expect("a manifest sourced from a live plugin registry must really spawn");
    let spawned = agent_runtime
        .describe(instance_id)
        .expect("the instance this test just spawned must really exist");
    assert_eq!(spawned.manifest.specialization, "custom-translator");
}
