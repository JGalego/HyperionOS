//! docs/11-agent-runtime.md §3.3/§6/§8: the lifecycle state machine, the
//! Broker's three-way grant resolution, the circuit breaker, and
//! checkpoint/resume revoking open grants.

use hyperion_agent_runtime::{
    AgentManifest, AgentRuntime, InvokeOutcome, LifecycleState, TrustTier,
};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use serde_json::json;

fn manifest() -> AgentManifest {
    AgentManifest {
        specialization: "research".to_string(),
        baseline_capabilities: vec!["web.search".to_string()],
        requestable_capabilities: vec!["document.draft".to_string()],
        trust_tier: TrustTier::System,
    }
}

fn setup() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    AgentRuntime,
) {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    (monitor, token, AgentRuntime::new())
}

#[test]
fn spawn_binds_immediately_and_baseline_capability_is_granted_without_a_prompt() {
    let (monitor, token, runtime) = setup();
    let id = runtime
        .spawn(&monitor, &token, manifest(), Some(42))
        .unwrap();
    assert_eq!(runtime.state_of(id), Some(LifecycleState::Bound));

    let outcome = runtime
        .invoke(
            &monitor,
            &token,
            id,
            "web.search",
            json!({"query": "rust ownership"}),
        )
        .unwrap();
    match outcome {
        InvokeOutcome::Result(value) => assert!(value["results"][0]
            .as_str()
            .unwrap()
            .contains("rust ownership")),
        other => panic!("expected Result, got {other:?}"),
    }
    assert_eq!(runtime.state_of(id), Some(LifecycleState::Executing));
}

#[test]
fn requestable_capability_blocks_on_consent_then_proceeds_once_approved() {
    let (monitor, token, runtime) = setup();
    let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();

    let outcome = runtime
        .invoke(&monitor, &token, id, "document.draft", json!({}))
        .unwrap();
    assert!(matches!(outcome, InvokeOutcome::PendingConsent));
    assert_eq!(
        runtime.state_of(id),
        Some(LifecycleState::WaitingOnCapability)
    );

    runtime.resolve_consent(&monitor, &token, id, true).unwrap();
    let outcome = runtime
        .invoke(
            &monitor,
            &token,
            id,
            "document.draft",
            json!({"topic": "roadmap"}),
        )
        .unwrap();
    assert!(matches!(outcome, InvokeOutcome::Result(_)));
}

#[test]
fn undeclared_capability_is_denied_unconditionally_no_prompt() {
    let (monitor, token, runtime) = setup();
    let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();

    let outcome = runtime
        .invoke(&monitor, &token, id, "payment.initiate", json!({}))
        .unwrap();
    assert!(matches!(outcome, InvokeOutcome::Denied));
    // Denial must never itself block the instance waiting on anything.
    assert_eq!(runtime.state_of(id), Some(LifecycleState::Bound));
}

#[test]
fn circuit_breaker_suspends_after_consecutive_failures_and_further_invokes_are_rejected() {
    let (monitor, token, runtime) = setup();
    let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();

    for _ in 0..2 {
        let outcome = runtime
            .invoke(
                &monitor,
                &token,
                id,
                "web.search",
                json!({"force_fail": true}),
            )
            .unwrap();
        assert!(matches!(outcome, InvokeOutcome::Failed(_)));
        assert_eq!(
            runtime.state_of(id),
            Some(LifecycleState::Executing),
            "not tripped yet"
        );
    }

    let outcome = runtime
        .invoke(
            &monitor,
            &token,
            id,
            "web.search",
            json!({"force_fail": true}),
        )
        .unwrap();
    assert!(matches!(outcome, InvokeOutcome::Failed(_)));
    assert_eq!(
        runtime.state_of(id),
        Some(LifecycleState::Suspended),
        "3rd consecutive failure trips the breaker"
    );

    let result = runtime.invoke(&monitor, &token, id, "web.search", json!({}));
    assert!(
        result.is_err(),
        "a suspended instance must reject further invocation"
    );
}

#[test]
fn a_success_resets_the_consecutive_failure_counter() {
    let (monitor, token, runtime) = setup();
    let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();

    runtime
        .invoke(
            &monitor,
            &token,
            id,
            "web.search",
            json!({"force_fail": true}),
        )
        .unwrap();
    runtime
        .invoke(
            &monitor,
            &token,
            id,
            "web.search",
            json!({"force_fail": true}),
        )
        .unwrap();
    runtime
        .invoke(&monitor, &token, id, "web.search", json!({"query": "ok"}))
        .unwrap(); // resets

    for _ in 0..2 {
        let outcome = runtime
            .invoke(
                &monitor,
                &token,
                id,
                "web.search",
                json!({"force_fail": true}),
            )
            .unwrap();
        assert!(matches!(outcome, InvokeOutcome::Failed(_)));
    }
    assert_eq!(
        runtime.state_of(id),
        Some(LifecycleState::Executing),
        "only 2 consecutive failures since the reset — breaker must not trip yet"
    );
}

#[test]
fn checkpoint_revokes_grants_and_resume_requires_re_consent() {
    let (monitor, token, runtime) = setup();
    let id = runtime
        .spawn(&monitor, &token, manifest(), Some(7))
        .unwrap();

    runtime
        .invoke(&monitor, &token, id, "document.draft", json!({}))
        .unwrap(); // PendingConsent
    runtime.resolve_consent(&monitor, &token, id, true).unwrap();
    runtime
        .invoke(
            &monitor,
            &token,
            id,
            "document.draft",
            json!({"topic": "x"}),
        )
        .unwrap(); // now granted

    let checkpoint_id = runtime.checkpoint(&monitor, &token, id).unwrap();
    assert_eq!(runtime.state_of(id), Some(LifecycleState::Checkpointed));
    assert!(
        runtime.describe(id).unwrap().grants.is_empty(),
        "checkpoint must revoke open grants"
    );

    let resumed_id = runtime.resume(&monitor, &token, checkpoint_id).unwrap();
    assert_eq!(resumed_id, id, "resume continues the same instance record");
    assert_eq!(runtime.state_of(id), Some(LifecycleState::Executing));

    // The grant was revoked at checkpoint time — re-invoking must re-ask.
    let outcome = runtime
        .invoke(&monitor, &token, id, "document.draft", json!({}))
        .unwrap();
    assert!(matches!(outcome, InvokeOutcome::PendingConsent));
}

#[test]
fn terminate_is_terminal_and_blocks_further_invocation() {
    let (monitor, token, runtime) = setup();
    let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();
    runtime
        .terminate(&monitor, &token, id, "user cancelled")
        .unwrap();
    assert_eq!(runtime.state_of(id), Some(LifecycleState::Terminated));
    assert!(runtime
        .invoke(&monitor, &token, id, "web.search", json!({}))
        .is_err());
}
