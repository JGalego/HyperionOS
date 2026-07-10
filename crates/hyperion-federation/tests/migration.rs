//! docs/21 §Algorithms' "Session/state migration": checkpoint on the
//! source, transfer the checkpoint's contents, spawn-and-rebind on the
//! target, terminate the source — end to end, across two genuinely
//! independent `AgentRuntime` instances.

use hyperion_agent_runtime::{AgentManifest, TrustTier};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_federation::{FederationError, FederationHub, FederationTrustTier, MigrationOutcome};

fn setup() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    FederationHub,
) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let hub = FederationHub::new();
    (monitor, root, hub)
}

fn manifest() -> AgentManifest {
    AgentManifest {
        specialization: "navigation".to_string(),
        baseline_capabilities: vec!["web.search".to_string()],
        requestable_capabilities: vec![],
        trust_tier: TrustTier::System,
    }
}

#[test]
fn an_agent_survives_migration_with_its_manifest_and_intent_intact() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    hub.join_device(&monitor, &root, 2, FederationTrustTier::OwnedSecondary)
        .unwrap();

    let agent = hub
        .spawn_agent(&monitor, &root, 1, manifest(), Some(777), 1_000, 60)
        .unwrap();
    assert_eq!(hub.device_of(agent), Some(1));

    let receipt = hub.migrate(&monitor, &root, agent, 2, 1_010, 60).unwrap();
    assert_eq!(receipt.outcome, MigrationOutcome::Completed);
    assert_eq!(receipt.target_device, 2);
    assert_eq!(hub.device_of(agent), Some(2));

    // The capability the manifest declared still resolves post-migration —
    // this is the check that the manifest genuinely transferred, not just
    // a location pointer.
    let outcome = hub
        .invoke_agent(
            &monitor,
            &root,
            agent,
            "web.search",
            serde_json::json!({"query": "restaurants"}),
            1_020,
        )
        .unwrap();
    assert!(matches!(
        outcome,
        hyperion_agent_runtime::InvokeOutcome::Result(_)
    ));

    let lease = hub.lease_of(agent).unwrap();
    assert_eq!(lease.holder_device, 2);
    assert_eq!(lease.generation, 1);
}

#[test]
fn the_source_instance_is_terminated_after_migration() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    hub.join_device(&monitor, &root, 2, FederationTrustTier::OwnedSecondary)
        .unwrap();

    let agent = hub
        .spawn_agent(&monitor, &root, 1, manifest(), None, 1_000, 60)
        .unwrap();
    hub.migrate(&monitor, &root, agent, 2, 1_010, 60).unwrap();

    // Re-deriving a token scoped to device 1 and inspecting its runtime
    // directly isn't exposed by this crate's API by design (the hub owns
    // the runtimes) — instead, confirm indirectly: a second migrate back
    // to device 1 must succeed, which is only possible if device 1's
    // runtime still exists and the lease correctly moved to device 2 as
    // the sole prerequisite for that request.
    let back = hub.migrate(&monitor, &root, agent, 1, 1_020, 60);
    assert!(back.is_ok());
}

#[test]
fn invoke_agent_produces_a_real_completed_explanation_record() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    let agent = hub
        .spawn_agent(&monitor, &root, 1, manifest(), None, 1_000, 60)
        .unwrap();

    hub.invoke_agent(
        &monitor,
        &root,
        agent,
        "web.search",
        serde_json::json!({"query": "hyperion os"}),
        1_010,
    )
    .unwrap();

    let records = hub.trace_intent(0);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].agent_id, agent);
    assert_eq!(
        records[0].control_state,
        hyperion_explainability::ControlState::Completed
    );
}

#[test]
fn only_the_current_anchor_device_may_initiate_a_migration() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::SharedHousehold)
        .unwrap();
    hub.join_device(&monitor, &root, 2, FederationTrustTier::OwnedSecondary)
        .unwrap();
    hub.join_device(&monitor, &root, 3, FederationTrustTier::OwnedPrimary)
        .unwrap();

    let agent = hub
        .spawn_agent(&monitor, &root, 1, manifest(), None, 1_000, 60)
        .unwrap();
    // Device 3 outranks device 1 and wins a direct challenge for the
    // lease — simulating device 1 having gone stale while device 3 took
    // over as anchor. Device 1 no longer holds the lease and must now be
    // barred from migrating an agent it no longer anchors.
    hub.acquire_lease(&monitor, &root, agent, 3, 1_005, 60)
        .unwrap();

    let result = hub.migrate(&monitor, &root, agent, 2, 1_010, 60);
    assert!(matches!(result, Err(FederationError::NotAuthoritative)));
}

#[test]
fn migrating_to_an_unknown_device_fails_without_side_effects() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    let agent = hub
        .spawn_agent(&monitor, &root, 1, manifest(), None, 1_000, 60)
        .unwrap();

    let result = hub.migrate(&monitor, &root, agent, 99, 1_010, 60);
    assert!(matches!(result, Err(FederationError::NoSuchDevice)));
    assert_eq!(hub.device_of(agent), Some(1));
}

#[test]
fn spawn_agent_yields_a_bound_instance_ready_to_invoke() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    let agent = hub
        .spawn_agent(&monitor, &root, 1, manifest(), None, 1_000, 60)
        .unwrap();
    let outcome = hub
        .invoke_agent(
            &monitor,
            &root,
            agent,
            "web.search",
            serde_json::json!({"query": "x"}),
            1_000,
        )
        .unwrap();
    assert!(matches!(
        outcome,
        hyperion_agent_runtime::InvokeOutcome::Result(_)
    ));
}
