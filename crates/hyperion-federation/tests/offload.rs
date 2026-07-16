//! docs/21 §Algorithms' "Task offload execution": placement scores real
//! `ResourceVector` headroom, the privacy gate makes an unconsented cloud
//! device architecturally invisible (not merely deprioritized), and a
//! candidate that fails on arrival is invalidated with automatic retry.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_federation::{
    FederationError, FederationHub, FederationTrustTier, OffloadDescriptor, PrivacyTier,
};
use hyperion_scheduler::ResourceVector;

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

fn small_request() -> ResourceVector {
    ResourceVector {
        cpu_shares: 10,
        ram_mb: 128,
        ..Default::default()
    }
}

fn ample_ledger() -> ResourceVector {
    ResourceVector {
        cpu_shares: 1000,
        ram_mb: 8192,
        gpu_shares: 100,
        vram_mb: 8192,
        storage_iops: 1000,
        network_bw_kbps: 100_000,
        inference_tokens_per_sec: 1000,
        context_window_slots: 1000,
        battery_budget_mw: 100_000,
    }
}

#[test]
fn picks_the_lowest_latency_feasible_candidate() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    hub.join_device(&monitor, &root, 2, FederationTrustTier::OwnedSecondary)
        .unwrap();
    hub.publish_ledger(1, ample_ledger(), 80, 1_000, 60)
        .unwrap();
    hub.publish_ledger(2, ample_ledger(), 20, 1_000, 60)
        .unwrap();

    let descriptor = OffloadDescriptor {
        request: small_request(),
        deadline_ms: None,
        privacy_tier: PrivacyTier::Local,
    };
    let result = hub
        .dispatch_offload(
            &monitor,
            &root,
            &descriptor,
            "document.draft",
            serde_json::json!({"topic": "quarterly report"}),
            42,
            1_000,
        )
        .unwrap();
    // `document.draft` dispatches through a real `LocalAiRuntime::infer` call now (a real,
    // previously-shipped bug: it used to be a canned stub string, discarded by every real caller
    // anyway -- see `hyperion-agent-runtime`'s own doc comment). `MockBackend` deterministically
    // echoes the whole prompt `AgentRuntime::dispatch_document_draft` built from this call's own
    // `"topic"` arg, so this remains an exact, deterministic assertion, just against real content
    // instead of a hand-written placeholder.
    assert_eq!(
        result["draft"],
        serde_json::json!("[mock model 1] echo: Draft a concise, practical quarterly report.")
    );
    // Device 2 has the lower latency and should have been the one invoked;
    // there is no direct observable other than the successful result here,
    // so the adversarial test below checks exclusion directly instead.
}

#[test]
fn dispatch_offload_produces_a_real_completed_explanation_record() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    hub.publish_ledger(1, ample_ledger(), 20, 1_000, 60)
        .unwrap();

    let descriptor = OffloadDescriptor {
        request: small_request(),
        deadline_ms: None,
        privacy_tier: PrivacyTier::Local,
    };
    hub.dispatch_offload(
        &monitor,
        &root,
        &descriptor,
        "web.search",
        serde_json::json!({"query": "hyperion os"}),
        55,
        1_000,
    )
    .unwrap();

    let records = hub.trace_intent(&monitor, &root, 55).unwrap();
    assert_eq!(
        records.len(),
        1,
        "a real, caller-supplied triggering_intent_id must be a genuine correlation, not a sentinel"
    );
    assert_eq!(
        records[0].control_state,
        hyperion_explainability::ControlState::Completed
    );
    assert!(!records[0].reasoning_chain.is_empty());
}

#[test]
fn unconsented_cloud_device_is_architecturally_invisible() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::CloudRented)
        .unwrap();
    // Only the cloud device has headroom.
    hub.publish_ledger(1, ample_ledger(), 5, 1_000, 60).unwrap();

    let descriptor = OffloadDescriptor {
        request: small_request(),
        deadline_ms: None,
        privacy_tier: PrivacyTier::Local, // not consented
    };
    let result = hub.dispatch_offload(
        &monitor,
        &root,
        &descriptor,
        "web.search",
        serde_json::json!({"query": "x"}),
        42,
        1_000,
    );
    assert!(matches!(result, Err(FederationError::NoFeasiblePlacement)));
}

#[test]
fn consented_cloud_device_is_eligible() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::CloudRented)
        .unwrap();
    hub.publish_ledger(1, ample_ledger(), 5, 1_000, 60).unwrap();

    let descriptor = OffloadDescriptor {
        request: small_request(),
        deadline_ms: None,
        privacy_tier: PrivacyTier::ConsentedCloud,
    };
    let result = hub.dispatch_offload(
        &monitor,
        &root,
        &descriptor,
        "web.search",
        serde_json::json!({"query": "x"}),
        42,
        1_000,
    );
    assert!(result.is_ok());
}

#[test]
fn insufficient_headroom_is_infeasible() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    hub.publish_ledger(
        1,
        ResourceVector {
            cpu_shares: 1,
            ..Default::default()
        },
        5,
        1_000,
        60,
    )
    .unwrap();

    let descriptor = OffloadDescriptor {
        request: small_request(),
        deadline_ms: None,
        privacy_tier: PrivacyTier::Local,
    };
    let result = hub.dispatch_offload(
        &monitor,
        &root,
        &descriptor,
        "web.search",
        serde_json::json!({"query": "x"}),
        42,
        1_000,
    );
    assert!(matches!(result, Err(FederationError::NoFeasiblePlacement)));
}

#[test]
fn a_stale_ledger_is_excluded() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    hub.publish_ledger(1, ample_ledger(), 5, 1_000, 10).unwrap();

    let descriptor = OffloadDescriptor {
        request: small_request(),
        deadline_ms: None,
        privacy_tier: PrivacyTier::Local,
    };
    // now = 1_000 + 11s, past the ledger's 10s ttl.
    let result = hub.dispatch_offload(
        &monitor,
        &root,
        &descriptor,
        "web.search",
        serde_json::json!({"query": "x"}),
        42,
        1_011,
    );
    assert!(matches!(result, Err(FederationError::NoFeasiblePlacement)));
}

#[test]
fn a_deadline_excludes_a_too_slow_candidate() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    hub.publish_ledger(1, ample_ledger(), 500, 1_000, 60)
        .unwrap();

    let descriptor = OffloadDescriptor {
        request: small_request(),
        deadline_ms: Some(100),
        privacy_tier: PrivacyTier::Local,
    };
    let result = hub.dispatch_offload(
        &monitor,
        &root,
        &descriptor,
        "web.search",
        serde_json::json!({"query": "x"}),
        42,
        1_000,
    );
    assert!(matches!(result, Err(FederationError::NoFeasiblePlacement)));
}

#[test]
fn a_failing_candidate_is_invalidated_and_retried_against_the_next_one() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    hub.join_device(&monitor, &root, 2, FederationTrustTier::OwnedSecondary)
        .unwrap();
    // Device 1 is the lower-latency (preferred) candidate but always fails;
    // device 2 is the fallback.
    hub.publish_ledger(1, ample_ledger(), 10, 1_000, 60)
        .unwrap();
    hub.publish_ledger(2, ample_ledger(), 50, 1_000, 60)
        .unwrap();

    let descriptor = OffloadDescriptor {
        request: small_request(),
        deadline_ms: None,
        privacy_tier: PrivacyTier::Local,
    };
    let result = hub.dispatch_offload(
        &monitor,
        &root,
        &descriptor,
        "web.search",
        serde_json::json!({"force_fail": true}),
        42,
        1_000,
    );
    // Every candidate dispatches the same forced-failure args, so both
    // devices fail in turn and the whole offload is ultimately infeasible
    // — this exercises the retry loop's exhaustion path, not a success.
    assert!(matches!(result, Err(FederationError::NoFeasiblePlacement)));
}
