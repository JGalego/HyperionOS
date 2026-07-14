//! docs/11-agent-runtime.md §3.3/§6/§8: the lifecycle state machine, the
//! Broker's three-way grant resolution, the circuit breaker, and
//! checkpoint/resume revoking open grants.

use std::sync::Arc;

use hyperion_agent_runtime::{
    AgentManifest, AgentRuntime, InvokeOutcome, LifecycleState, TrustTier,
};
use hyperion_ai_runtime::{
    sign, InferenceBackend, InferenceRequest, LocalAiRuntime, MockBackend, ModelClass,
    ModelDescriptor, Precision, QuantizedVariant,
};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
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
    setup_with_backend(Box::new(MockBackend))
}

fn setup_with_backend(
    backend: Box<dyn InferenceBackend>,
) -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    AgentRuntime,
) {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let ai_runtime = Arc::new(LocalAiRuntime::new(backend, 8_000));

    // A real, signed ModelDescriptor -- needed now that `document.draft`/`web.search` really
    // call `LocalAiRuntime::infer` (see `AgentRuntime::dispatch_document_draft`/
    // `dispatch_market_research`), which fails closed with no model registered for the
    // requested `ModelClass`, exactly like `assistant.respond` always required.
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

/// A real `InferenceBackend` that takes a controlled, real amount of wall-clock time to
/// `generate` -- standing in for a real, slow network round trip to a real cloud model, without
/// this test needing one. See `concurrent_invokes_against_the_same_instance_genuinely_overlap_
/// not_serialize` below, the one test in this file that actually needs real elapsed time to mean
/// something.
struct SlowBackend {
    delay: std::time::Duration,
}

impl InferenceBackend for SlowBackend {
    fn generate(&self, _model_id: u64, request: &InferenceRequest) -> String {
        std::thread::sleep(self.delay);
        format!("slow echo: {}", request.prompt)
    }
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
fn invoke_round_trips_through_the_real_scheduler_without_leaking_capacity() {
    let (monitor, token, runtime) = setup();
    let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();

    let headroom_before = runtime.resource_headroom();
    assert!(
        headroom_before > 0,
        "a freshly constructed runtime's real Scheduler ledger must start with headroom"
    );

    runtime
        .invoke(
            &monitor,
            &token,
            id,
            "web.search",
            json!({"query": "rust ownership"}),
        )
        .unwrap();

    assert_eq!(
        runtime.resource_headroom(),
        headroom_before,
        "a completed invocation must release its real Scheduler reservation, not leak it"
    );
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
fn grant_capability_seeds_a_grant_with_no_live_pending_consent_request() {
    // docs/998-roadmap.md "Phase 2: cloud providers": proves the real seeding path a
    // console uses at startup for a provider whose secret is already stored -- unlike
    // `resolve_consent`, this never needs a live `PendingConsent` to have fired first.
    let (monitor, token, runtime) = setup();
    let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();

    // No invoke of "document.draft" has happened yet -- no PendingConsent is pending.
    runtime
        .grant_capability(&monitor, &token, id, "document.draft")
        .expect("seed a grant with no prior PendingConsent");

    let outcome = runtime
        .invoke(
            &monitor,
            &token,
            id,
            "document.draft",
            json!({"topic": "roadmap"}),
        )
        .unwrap();
    assert!(
        matches!(outcome, InvokeOutcome::Result(_)),
        "a directly-seeded grant must resolve Granted on the very first invoke, not \
         PendingConsent, got: {outcome:?}"
    );
}

#[test]
fn resolve_consent_still_requires_a_live_pending_request() {
    let (monitor, token, runtime) = setup();
    let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();

    let result = runtime.resolve_consent(&monitor, &token, id, true);
    assert!(
        result.is_err(),
        "resolving consent with no live PendingConsent request must fail, not silently \
         grant anything"
    );
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

/// Regression coverage for a real, previously-shipped bug: `document.draft`/`web.search` used to
/// dispatch to `stubs::dispatch`'s two hand-written canned strings (`"Stub draft document about
/// '...'."`/`"stub finding for query '...'"`) -- real now, via the same `LocalAiRuntime` call
/// `assistant.respond` already used. These prove the *real* dispatch is genuinely reached, not
/// just that some string comes back: `MockBackend::generate` echoes its whole prompt verbatim
/// (`"[mock model N] echo: <prompt>"`), so the topic/subject text must show up inside a real,
/// distinctly-shaped sentence -- something a canned stub could never produce.
mod real_content_generation {
    use super::*;

    #[test]
    fn document_draft_generates_real_content_via_the_real_inference_backend() {
        let (monitor, token, runtime) = setup();
        let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();
        runtime
            .grant_capability(&monitor, &token, id, "document.draft")
            .unwrap();

        let outcome = runtime
            .invoke(
                &monitor,
                &token,
                id,
                "document.draft",
                json!({"topic": "quarterly business model"}),
            )
            .unwrap();
        let InvokeOutcome::Result(value) = outcome else {
            panic!("expected Result, got {outcome:?}");
        };
        let draft = value["draft"].as_str().expect("a real \"draft\" string");
        assert!(
            draft.contains("Draft a concise, practical")
                && draft.contains("quarterly business model"),
            "expected real, prompt-driven generation (MockBackend echoes its own prompt), not a \
             canned stub string, got: {draft:?}"
        );
        assert_ne!(
            draft, "Stub draft document about 'quarterly business model'.",
            "must not still be the old hand-written stub text"
        );
    }

    /// The real "redo this with more information" verb `hyperion-coordination::
    /// CoordinationSession::amend_task` sets before a task's next real dispatch -- proves it
    /// genuinely reaches the real prompt, not just that the args key exists.
    #[test]
    fn extra_context_when_present_shows_up_in_the_real_generated_prompt() {
        let (monitor, token, runtime) = setup();
        let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();
        runtime
            .grant_capability(&monitor, &token, id, "document.draft")
            .unwrap();

        let outcome = runtime
            .invoke(
                &monitor,
                &token,
                id,
                "document.draft",
                json!({"topic": "business model", "extra_context": "focus on Europe only"}),
            )
            .unwrap();
        let InvokeOutcome::Result(value) = outcome else {
            panic!("expected Result, got {outcome:?}");
        };
        let draft = value["draft"].as_str().unwrap();
        assert!(draft.contains("focus on Europe only"), "got: {draft:?}");
    }

    #[test]
    fn web_search_generates_real_content_and_is_honest_about_not_being_a_live_search() {
        let (monitor, token, runtime) = setup();
        let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();

        let outcome = runtime
            .invoke(
                &monitor,
                &token,
                id,
                "web.search",
                json!({"query": "total addressable market for pet robots"}),
            )
            .unwrap();
        let InvokeOutcome::Result(value) = outcome else {
            panic!("expected Result, got {outcome:?}");
        };
        let result_text = value["results"][0]
            .as_str()
            .expect("a real \"results\" entry");
        assert!(
            result_text.contains("total addressable market for pet robots"),
            "expected real, prompt-driven generation, got: {result_text:?}"
        );
        assert_eq!(
            value["note"], "AI-generated research notes, not a live web search",
            "this workspace has no real search-provider integration -- the result must say so \
             plainly rather than let a caller mistake it for a verified live search"
        );
    }

    #[test]
    fn document_draft_still_honors_force_fail_for_the_circuit_breaker_test_seam() {
        let (monitor, token, runtime) = setup();
        let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();
        runtime
            .grant_capability(&monitor, &token, id, "document.draft")
            .unwrap();

        let outcome = runtime
            .invoke(
                &monitor,
                &token,
                id,
                "document.draft",
                json!({"force_fail": true}),
            )
            .unwrap();
        assert!(
            matches!(outcome, InvokeOutcome::Failed(_)),
            "the real dispatch must still honor force_fail, or hyperion-coordination's own \
             retry/escalation tests (which inject exactly this) would silently break"
        );
    }
}

/// Regression coverage for a real, previously-shipped bottleneck: `invoke` used to hold
/// `self.instances`' single global lock across the *entire* call, including the real capability
/// dispatch -- so two concurrent `invoke` calls, even against two different instances, would
/// serialize behind one lock the instant either reached a real, slow dispatch. Fixed by splitting
/// `invoke` into a locked "prepare" phase and an unlocked "dispatch" phase (see that function's
/// own doc comment). Proven here against the *same* instance specifically -- the harder, more
/// realistic case `hyperion-coordination::allocate` actually hits (one writer instance handles
/// `business_model`/`branding`/`legal_formation`; see that crate's own "one research + one writer
/// instance, reused across tasks" test).
#[test]
fn concurrent_invokes_against_the_same_instance_genuinely_overlap_not_serialize() {
    let (monitor, token, runtime) = setup_with_backend(Box::new(SlowBackend {
        delay: std::time::Duration::from_millis(200),
    }));
    let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();
    runtime
        .grant_capability(&monitor, &token, id, "document.draft")
        .unwrap();

    let start = std::time::Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..2 {
            scope.spawn(|| {
                runtime
                    .invoke(
                        &monitor,
                        &token,
                        id,
                        "document.draft",
                        json!({"topic": "x"}),
                    )
                    .unwrap();
            });
        }
    });
    let elapsed = start.elapsed();

    assert!(
        elapsed < std::time::Duration::from_millis(350),
        "two real 200ms dispatches against the SAME agent instance took {elapsed:?} -- expected \
         them to genuinely overlap (~200ms total if they run concurrently), not serialize behind \
         one global lock (~400ms total if they don't)"
    );
}
