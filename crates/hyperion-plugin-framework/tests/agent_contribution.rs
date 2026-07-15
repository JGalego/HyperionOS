//! docs/998-roadmap.md's Resourceful pillar: `Contribution::Agent` is a real, live registration
//! point, not a hardcoded static list — a plugin can install an agent specialization, have it
//! show up through `PluginRegistry::agent_contributions`, and have it disappear again on
//! uninstall or quarantine, exactly like a `Capability` contribution already does.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{
    sign, AgentContribution, CapabilityGrantRequest, Contribution, Operation, PluginManifest,
    PluginRegistry, QuarantineReason, TrustDepth,
};

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

fn manifest_with_agent(keystore: &Keystore, plugin_id: u64) -> PluginManifest {
    let mut manifest = PluginManifest {
        plugin_id,
        publisher: "acme-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Agent(AgentContribution {
            specialization: "translator".to_string(),
            baseline_capabilities: vec!["document.translate".to_string()],
            requestable_capabilities: vec![],
        })],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::Execute,
            scope: "translator".to_string(),
            justification: "the agent must be dispatchable".to_string(),
        }],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = Some(sign(&manifest, keystore));
    manifest
}

#[test]
fn installing_an_agent_contribution_makes_it_really_discoverable() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    registry
        .install(
            &mut monitor,
            &root,
            manifest_with_agent(&keystore, 1),
            TrustDepth::D0,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let agents = registry.agent_contributions();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].specialization, "translator");
    assert_eq!(agents[0].baseline_capabilities, vec!["document.translate"]);
}

#[test]
fn a_network_egress_request_is_never_justified_by_an_agent_contribution_alone() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let mut manifest = manifest_with_agent(&keystore, 1);
    manifest.requested_permissions = vec![CapabilityGrantRequest {
        operation: Operation::NetworkEgress,
        scope: "translator".to_string(),
        justification: "an agent contribution alone can't justify this".to_string(),
    }];
    manifest.signature = Some(sign(&manifest, &keystore));

    let result = registry.install(
        &mut monitor,
        &root,
        manifest,
        TrustDepth::D0,
        true,
        1_000,
        &keystore.verifying_key(),
    );
    assert!(
        result.is_err(),
        "NetworkEgress must never be smuggled in behind a bare Agent contribution"
    );
}

#[test]
fn uninstalling_removes_its_agent_contributions() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let handle = registry
        .install(
            &mut monitor,
            &root,
            manifest_with_agent(&keystore, 1),
            TrustDepth::D0,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();
    assert_eq!(registry.agent_contributions().len(), 1);

    registry
        .uninstall(&mut monitor, &root, handle.plugin_id)
        .unwrap();
    assert!(registry.agent_contributions().is_empty());
}

#[test]
fn quarantining_an_agent_only_plugin_hides_its_contributions_without_uninstalling() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let handle = registry
        .install(
            &mut monitor,
            &root,
            manifest_with_agent(&keystore, 1),
            TrustDepth::D0,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();
    assert_eq!(registry.agent_contributions().len(), 1);

    registry
        .quarantine(handle.plugin_id, QuarantineReason::PolicyViolation)
        .unwrap();
    assert!(
        registry.agent_contributions().is_empty(),
        "a quarantined plugin's agent contributions must not be returned as eligible"
    );
}

#[test]
fn two_plugins_each_contributing_an_agent_are_both_discoverable_and_independently_removable() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir1, keystore1) = keystore();
    let (_dir2, keystore2) = keystore();

    let mut second = manifest_with_agent(&keystore2, 2);
    second.contributions = vec![Contribution::Agent(AgentContribution {
        specialization: "summarizer".to_string(),
        baseline_capabilities: vec!["document.summarize".to_string()],
        requestable_capabilities: vec![],
    })];
    second.signature = Some(sign(&second, &keystore2));

    let handle1 = registry
        .install(
            &mut monitor,
            &root,
            manifest_with_agent(&keystore1, 1),
            TrustDepth::D0,
            true,
            1_000,
            &keystore1.verifying_key(),
        )
        .unwrap();
    registry
        .install(
            &mut monitor,
            &root,
            second,
            TrustDepth::D0,
            true,
            1_000,
            &keystore2.verifying_key(),
        )
        .unwrap();

    let mut specializations: Vec<String> = registry
        .agent_contributions()
        .into_iter()
        .map(|a| a.specialization)
        .collect();
    specializations.sort();
    assert_eq!(
        specializations,
        vec!["summarizer".to_string(), "translator".to_string()]
    );

    registry
        .uninstall(&mut monitor, &root, handle1.plugin_id)
        .unwrap();
    let remaining = registry.agent_contributions();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].specialization, "summarizer");
}
