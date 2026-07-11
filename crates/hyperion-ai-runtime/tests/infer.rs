//! docs/22-local-ai-runtime.md's `load_and_infer` pseudocode: capability-
//! gated invocation, §5.3's power-mode downgrade, and the infeasible-
//! locally signal when nothing fits.

use hyperion_ai_runtime::{
    sign, CapabilityContract, InferenceRequest, LocalAiRuntime, MockBackend, ModelClass,
    ModelDescriptor, PowerMode, Precision, QuantizedVariant, RuntimeError,
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
