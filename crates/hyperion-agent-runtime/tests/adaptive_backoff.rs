//! docs/998-roadmap.md's Self-Sustaining pillar: a `Suspended` `AgentInstance` has a real path
//! back -- it auto-resumes after a real, adaptive backoff window instead of staying stuck until
//! something external intervenes, and a real repeat-offense history makes that backoff longer
//! next time, decaying back down after a real streak of successes. Real, short wall-clock waits
//! throughout: this crate has no clock-injection seam, and `tests/lifecycle.rs`'s own
//! `concurrent_invokes_against_the_same_instance_genuinely_overlap_not_serialize` test already
//! established real-sleep as this crate's own convention rather than adding one just for this.

use std::sync::Arc;

use hyperion_agent_runtime::{
    AgentManifest, AgentRuntime, InvokeOutcome, LifecycleState, TrustTier,
};
use hyperion_ai_runtime::{
    sign, LocalAiRuntime, MockBackend, ModelClass, ModelDescriptor, Precision, QuantizedVariant,
};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use serde_json::json;

fn manifest() -> AgentManifest {
    AgentManifest {
        specialization: "research".to_string(),
        baseline_capabilities: vec!["web.search".to_string()],
        requestable_capabilities: vec![],
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
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));

    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    let mut descriptor = ModelDescriptor {
        model_id: 1,
        class: ModelClass::Slm,
        variants: vec![QuantizedVariant {
            precision: Precision::Fp16,
            footprint_mb: 100,
            expected_tokens_per_sec: 10.0,
        }],
        signature: None,
    };
    descriptor.signature = Some(sign(&descriptor, &keystore));
    ai_runtime
        .register_model(descriptor, &keystore.verifying_key())
        .expect("a descriptor this test just signed always verifies");

    (monitor, token, AgentRuntime::new(ai_runtime))
}

/// Drives exactly `CIRCUIT_BREAKER_THRESHOLD`-worth (3) of real forced failures against `id`,
/// tripping the breaker for real.
fn trip_the_breaker(
    monitor: &CapabilityMonitor,
    token: &hyperion_capability::CapabilityToken,
    runtime: &AgentRuntime,
    id: u64,
) {
    for _ in 0..3 {
        runtime
            .invoke(
                monitor,
                token,
                id,
                "web.search",
                json!({"force_fail": true}),
            )
            .unwrap();
    }
}

#[test]
fn an_immediate_retry_after_suspension_gets_an_honest_still_recovering_message() {
    let (monitor, token, runtime) = setup();
    let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();
    trip_the_breaker(&monitor, &token, &runtime, id);
    assert_eq!(runtime.state_of(id), Some(LifecycleState::Suspended));

    let result = runtime.invoke(&monitor, &token, id, "web.search", json!({}));
    let Err(e) = result else {
        panic!("expected an immediate retry to still be rejected, got: {result:?}");
    };
    let message = e.to_string();
    assert!(
        message.contains("recovering") && message.contains("try again"),
        "expected an honest, human-facing message, not a bare technical error, got: {message:?}"
    );
}

#[test]
fn after_the_real_backoff_window_elapses_the_instance_auto_resumes_and_actually_runs() {
    let (monitor, token, runtime) = setup();
    let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();
    trip_the_breaker(&monitor, &token, &runtime, id);

    // A first suspension's real backoff is 1 real second (`backoff_duration(1)`) -- wait past it.
    std::thread::sleep(std::time::Duration::from_millis(1_100));

    let outcome = runtime
        .invoke(&monitor, &token, id, "web.search", json!({}))
        .expect("a real auto-resumed invoke must actually proceed, not error");
    assert!(
        matches!(outcome, InvokeOutcome::Result(_)),
        "expected the real dispatch to actually run post-resume, got: {outcome:?}"
    );
    assert_eq!(
        runtime.state_of(id),
        Some(LifecycleState::Executing),
        "a real auto-resumed instance must actually be running again, not stuck"
    );

    let instance = runtime.describe(id).unwrap();
    assert_eq!(
        instance.quota.times_suspended, 1,
        "one real suspension so far"
    );
    assert!(
        instance
            .audit_log
            .iter()
            .any(|e| e.kind == "auto_resumed_after_backoff"),
        "the real auto-resume must be a real, explainable, audited event"
    );
}

#[test]
fn a_second_suspensions_backoff_is_measurably_longer_than_the_first() {
    let (monitor, token, runtime) = setup();
    let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();

    // First suspension: backoff_duration(1) == 1s.
    trip_the_breaker(&monitor, &token, &runtime, id);
    std::thread::sleep(std::time::Duration::from_millis(1_100));
    runtime
        .invoke(&monitor, &token, id, "web.search", json!({}))
        .unwrap();
    assert_eq!(runtime.describe(id).unwrap().quota.times_suspended, 1);

    // Trip it again -- times_suspended becomes 2, so the *next* backoff is backoff_duration(2) ==
    // 2s. A retry after only 1.1s (which fully covered the *first* backoff) must still be
    // rejected -- the real, adaptive, "this instance is a repeat offender" signal.
    trip_the_breaker(&monitor, &token, &runtime, id);
    assert_eq!(runtime.describe(id).unwrap().quota.times_suspended, 2);
    std::thread::sleep(std::time::Duration::from_millis(1_100));
    let too_early = runtime.invoke(&monitor, &token, id, "web.search", json!({}));
    assert!(
        too_early.is_err(),
        "1.1s must not be enough for a second suspension's real, longer (2s) backoff, got: \
         {too_early:?}"
    );

    // The rest of the real, longer backoff elapses -- now it really does resume.
    std::thread::sleep(std::time::Duration::from_millis(1_000));
    let outcome = runtime
        .invoke(&monitor, &token, id, "web.search", json!({}))
        .expect("the real, longer backoff must have elapsed by now");
    assert!(matches!(outcome, InvokeOutcome::Result(_)));
}

#[test]
fn a_real_success_streak_after_resume_decays_times_suspended_back_down() {
    let (monitor, token, runtime) = setup();
    let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();

    trip_the_breaker(&monitor, &token, &runtime, id);
    std::thread::sleep(std::time::Duration::from_millis(1_100));

    // 3 real, consecutive successes after the resume -- SUCCESS_STREAK_TO_DECAY.
    for _ in 0..3 {
        let outcome = runtime
            .invoke(&monitor, &token, id, "web.search", json!({}))
            .unwrap();
        assert!(matches!(outcome, InvokeOutcome::Result(_)));
    }

    let instance = runtime.describe(id).unwrap();
    assert_eq!(
        instance.quota.times_suspended, 0,
        "a real, sustained success streak must earn the backoff back down to zero"
    );
    assert!(
        instance
            .audit_log
            .iter()
            .any(|e| e.kind == "backoff_decayed"),
        "the real decay must itself be a real, explainable, audited event"
    );
}
