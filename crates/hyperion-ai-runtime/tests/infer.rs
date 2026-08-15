//! docs/22-local-ai-runtime.md's `load_and_infer` pseudocode: capability-
//! gated invocation, §5.3's power-mode downgrade, and the infeasible-
//! locally signal when nothing fits.

use hyperion_ai_runtime::{
    sign, CancellationToken, CapabilityContract, InferenceBackend, InferenceRequest,
    LocalAiRuntime, MockBackend, ModelClass, ModelDescriptor, PowerMode, Precision,
    QuantizedVariant, RuntimeError,
};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

fn descriptor_with_two_tiers(keystore: &Keystore) -> ModelDescriptor {
    let mut d = ModelDescriptor {
        model_id: 1,
        class: ModelClass::Slm,
        variants: vec![
            QuantizedVariant {
                precision: Precision::Fp16,
                footprint_mb: 4_000,
                expected_tokens_per_sec: 50.0,
            },
            QuantizedVariant {
                precision: Precision::Int4,
                footprint_mb: 500,
                expected_tokens_per_sec: 60.0,
            },
        ],
        signature: None,
    };
    d.signature = Some(sign(&d, keystore));
    d
}

#[test]
fn infer_requires_exec_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();

    let (_dir, keystore) = keystore();
    let runtime = LocalAiRuntime::new(Box::new(MockBackend), 8_000);
    runtime
        .register_model(
            descriptor_with_two_tiers(&keystore),
            &keystore.verifying_key(),
        )
        .unwrap();

    let contract = CapabilityContract {
        latency_budget_ms: 5_000,
        always_on: false,
    };
    let result = runtime.infer(
        &monitor,
        &read_only,
        ModelClass::Slm,
        &contract,
        &InferenceRequest {
            prompt: "hello".to_string(),
        },
    );
    assert!(matches!(result, Err(RuntimeError::Unauthorized)));
}

#[test]
fn infer_returns_a_deterministic_mock_response() {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let (_dir, keystore) = keystore();
    let runtime = LocalAiRuntime::new(Box::new(MockBackend), 8_000);
    runtime
        .register_model(
            descriptor_with_two_tiers(&keystore),
            &keystore.verifying_key(),
        )
        .unwrap();

    let contract = CapabilityContract {
        latency_budget_ms: 5_000,
        always_on: false,
    };
    let result = runtime
        .infer(
            &monitor,
            &token,
            ModelClass::Slm,
            &contract,
            &InferenceRequest {
                prompt: "hello".to_string(),
            },
        )
        .unwrap();
    assert!(result.text.contains("hello"));
    assert_eq!(result.variant_used, Precision::Fp16);
}

#[test]
fn battery_saver_mode_forces_the_smallest_variant_even_if_the_best_one_fits() {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let (_dir, keystore) = keystore();
    let runtime = LocalAiRuntime::new(Box::new(MockBackend), 8_000);
    runtime
        .register_model(
            descriptor_with_two_tiers(&keystore),
            &keystore.verifying_key(),
        )
        .unwrap();
    runtime.set_power_mode(PowerMode::BatterySaver);

    let contract = CapabilityContract {
        latency_budget_ms: 5_000,
        always_on: false,
    };
    let result = runtime
        .infer(
            &monitor,
            &token,
            ModelClass::Slm,
            &contract,
            &InferenceRequest {
                prompt: "hello".to_string(),
            },
        )
        .unwrap();
    assert_eq!(
        result.variant_used,
        Precision::Int4,
        "BatterySaver must downgrade, not use Fp16"
    );
}

#[test]
fn no_registered_model_for_the_class_is_infeasible_not_a_panic() {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let runtime = LocalAiRuntime::new(Box::new(MockBackend), 8_000);

    let contract = CapabilityContract {
        latency_budget_ms: 5_000,
        always_on: false,
    };
    let result = runtime.infer(
        &monitor,
        &token,
        ModelClass::Vision,
        &contract,
        &InferenceRequest {
            prompt: "hello".to_string(),
        },
    );
    assert!(matches!(result, Err(RuntimeError::InfeasibleLocally)));
}

/// A real `InferenceBackend` that takes a controlled, real amount of wall-clock time -- standing
/// in for a real, slow network round trip to a real cloud model. See the concurrency test below.
/// Every real call's own `(start, end)`, so a test can ask whether two calls genuinely overlapped
/// instead of inferring it from how long the pair took in total.
type DispatchSpans =
    std::sync::Arc<std::sync::Mutex<Vec<(std::time::Instant, std::time::Instant)>>>;

struct SlowBackend {
    delay: std::time::Duration,
    spans: DispatchSpans,
}

impl InferenceBackend for SlowBackend {
    fn generate(
        &self,
        _model_id: u64,
        request: &InferenceRequest,
        _cancel: &CancellationToken,
    ) -> String {
        let start = std::time::Instant::now();
        std::thread::sleep(self.delay);
        self.spans
            .lock()
            .unwrap()
            .push((start, std::time::Instant::now()));
        format!("slow echo: {}", request.prompt)
    }
}

/// A real `InferenceBackend` with a genuine per-step loop that actually consults `cancel` --
/// standing in for [`hyperion_ai_runtime::CandleBackend`]'s own real per-token check without
/// this test needing the `candle` feature (and its real network download) to prove the
/// runtime's own `in_flight` registry/`cancel()` plumbing reaches the token `generate` sees.
struct StepCountingBackend {
    step_delay: std::time::Duration,
    max_steps: u32,
}

impl InferenceBackend for StepCountingBackend {
    fn generate(
        &self,
        _model_id: u64,
        _request: &InferenceRequest,
        cancel: &CancellationToken,
    ) -> String {
        let mut steps_run = 0;
        for _ in 0..self.max_steps {
            if cancel.is_cancelled() {
                break;
            }
            std::thread::sleep(self.step_delay);
            steps_run += 1;
        }
        format!("ran {steps_run} of {} steps", self.max_steps)
    }
}

/// Proves `LocalAiRuntime::cancel` is real, not the previous no-op stub: a caller-supplied
/// `request_id` registered via `infer_cancellable` is looked up and its token flipped by a
/// concurrent `cancel(request_id)` call, and the backend -- which genuinely consults that same
/// token once per step -- stops early with fewer steps run than `max_steps`.
#[test]
fn cancel_stops_a_real_in_flight_request_before_it_reaches_max_steps() {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let (_dir, keystore) = keystore();
    let runtime = LocalAiRuntime::new(
        Box::new(StepCountingBackend {
            step_delay: std::time::Duration::from_millis(50),
            max_steps: 100,
        }),
        8_000,
    );
    runtime
        .register_model(
            descriptor_with_two_tiers(&keystore),
            &keystore.verifying_key(),
        )
        .unwrap();

    let contract = CapabilityContract {
        latency_budget_ms: 5_000,
        always_on: false,
    };
    let request = InferenceRequest {
        prompt: "hello".to_string(),
    };
    let request_id = 42;

    std::thread::scope(|scope| {
        let handle = scope.spawn(|| {
            runtime
                .infer_cancellable(
                    request_id,
                    &monitor,
                    &token,
                    ModelClass::Slm,
                    &contract,
                    &request,
                )
                .unwrap()
        });

        std::thread::sleep(std::time::Duration::from_millis(150));
        runtime.cancel(request_id);

        let result = handle.join().unwrap();
        let steps_run: u32 = result
            .text
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            steps_run < 100,
            "expected cancel() to stop generation well before all 100 steps ran, but {steps_run} ran"
        );
    });
}

/// A cancelled `request_id` that's already finished (or never existed) is a harmless no-op --
/// mirrors real-world races between a caller's cancel request and the backend finishing first.
#[test]
fn cancel_on_an_unknown_request_id_is_a_harmless_no_op() {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let (_dir, keystore) = keystore();
    let runtime = LocalAiRuntime::new(Box::new(MockBackend), 8_000);
    runtime
        .register_model(
            descriptor_with_two_tiers(&keystore),
            &keystore.verifying_key(),
        )
        .unwrap();

    runtime.cancel(9999);

    let contract = CapabilityContract {
        latency_budget_ms: 5_000,
        always_on: false,
    };
    let result = runtime
        .infer(
            &monitor,
            &token,
            ModelClass::Slm,
            &contract,
            &InferenceRequest {
                prompt: "hello".to_string(),
            },
        )
        .unwrap();
    assert!(result.text.contains("hello"));
}

/// Regression coverage for a real, previously-shipped bottleneck: `infer` used to hold the
/// `backend` mutex across the entire real `generate` call, so two concurrent `infer` calls would
/// serialize behind it no matter how many real OS threads a caller spawned -- the same class of
/// bug `hyperion_agent_runtime::AgentRuntime::invoke`'s own three-phase split fixes one layer up.
/// Fixed by storing an `Arc<dyn InferenceBackend>` behind the mutex instead of a bare `Box`, so
/// `infer` clones the `Arc` (cheap) and drops the lock before ever calling `generate`.
///
/// Asserted as overlapping time spans rather than a wall-clock budget: a budget makes the result
/// depend on machine load, and the same shape elsewhere in this workspace failed on a loaded CI
/// runner while behaving exactly as designed. Two spans are disjoint under serialization however
/// fast or slow the machine is.
#[test]
fn concurrent_infer_calls_genuinely_overlap_not_serialize() {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let (_dir, keystore) = keystore();
    let spans: DispatchSpans = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let runtime = LocalAiRuntime::new(
        Box::new(SlowBackend {
            delay: std::time::Duration::from_millis(200),
            spans: std::sync::Arc::clone(&spans),
        }),
        8_000,
    );
    runtime
        .register_model(
            descriptor_with_two_tiers(&keystore),
            &keystore.verifying_key(),
        )
        .unwrap();

    let contract = CapabilityContract {
        latency_budget_ms: 5_000,
        always_on: false,
    };
    let request = InferenceRequest {
        prompt: "hello".to_string(),
    };

    std::thread::scope(|scope| {
        for _ in 0..2 {
            scope.spawn(|| {
                runtime
                    .infer(&monitor, &token, ModelClass::Slm, &contract, &request)
                    .unwrap();
            });
        }
    });

    let spans = spans.lock().unwrap();
    assert_eq!(
        spans.len(),
        2,
        "both real infer calls must have reached the backend"
    );
    let (a_start, a_end) = spans[0];
    let (b_start, b_end) = spans[1];
    assert!(
        a_start < b_end && b_start < a_end,
        "two real infer calls serialized behind the backend lock rather than overlapping: \
         {:?} and {:?}",
        a_end.duration_since(a_start),
        b_end.duration_since(b_start),
    );
}
