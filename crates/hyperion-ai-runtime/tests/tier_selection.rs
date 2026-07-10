//! docs/22-local-ai-runtime.md §5.1: best variant first, stepping down on a
//! failed fit or latency check; §Failure Modes' "out of memory" — nothing
//! fits at all.

use hyperion_ai_runtime::{
    CapabilityContract, LocalAiRuntime, MockBackend, ModelClass, ModelDescriptor, Precision,
    QuantizedVariant,
};

fn descriptor(
    model_id: u64,
    class: ModelClass,
    variants: Vec<QuantizedVariant>,
) -> ModelDescriptor {
    let mut d = ModelDescriptor {
        model_id,
        class,
        variants,
        checksum: 0,
    };
    d.checksum = hyperion_ai_runtime::checksum(&d);
    d
}

#[test]
fn selects_the_best_fitting_variant_first() {
    let runtime = LocalAiRuntime::new(Box::new(MockBackend), 8_000);
    runtime
        .register_model(descriptor(
            1,
            ModelClass::Slm,
            vec![
                QuantizedVariant {
                    precision: Precision::Fp16,
                    footprint_mb: 4_000,
                    expected_tokens_per_sec: 50.0,
                },
                QuantizedVariant {
                    precision: Precision::Int4,
                    footprint_mb: 1_000,
                    expected_tokens_per_sec: 80.0,
                },
            ],
        ))
        .unwrap();

    let contract = CapabilityContract {
        latency_budget_ms: 5_000,
        always_on: false,
    };
    let (model_id, variant) = runtime.select_variant(ModelClass::Slm, &contract).unwrap();
    assert_eq!(model_id, 1);
    assert_eq!(variant.precision, Precision::Fp16);
}

#[test]
fn steps_down_when_the_best_variant_does_not_fit_this_device() {
    let runtime = LocalAiRuntime::new(Box::new(MockBackend), 2_000);
    runtime
        .register_model(descriptor(
            1,
            ModelClass::Slm,
            vec![
                QuantizedVariant {
                    precision: Precision::Fp16,
                    footprint_mb: 4_000, // doesn't fit an 2000mb device
                    expected_tokens_per_sec: 50.0,
                },
                QuantizedVariant {
                    precision: Precision::Int4,
                    footprint_mb: 1_000,
                    expected_tokens_per_sec: 80.0,
                },
            ],
        ))
        .unwrap();

    let contract = CapabilityContract {
        latency_budget_ms: 5_000,
        always_on: false,
    };
    let (_, variant) = runtime.select_variant(ModelClass::Slm, &contract).unwrap();
    assert_eq!(variant.precision, Precision::Int4);
}

#[test]
fn steps_down_when_the_best_variant_is_too_slow_for_the_latency_budget() {
    let runtime = LocalAiRuntime::new(Box::new(MockBackend), 8_000);
    runtime
        .register_model(descriptor(
            1,
            ModelClass::Slm,
            vec![
                QuantizedVariant {
                    precision: Precision::Fp16,
                    footprint_mb: 1_000,
                    expected_tokens_per_sec: 1.0, // ~100 seconds for 100 tokens
                },
                QuantizedVariant {
                    precision: Precision::Int4,
                    footprint_mb: 500,
                    expected_tokens_per_sec: 200.0,
                },
            ],
        ))
        .unwrap();

    let contract = CapabilityContract {
        latency_budget_ms: 1_000,
        always_on: false,
    };
    let (_, variant) = runtime.select_variant(ModelClass::Slm, &contract).unwrap();
    assert_eq!(variant.precision, Precision::Int4);
}

#[test]
fn nothing_fits_returns_none_not_a_panic() {
    let runtime = LocalAiRuntime::new(Box::new(MockBackend), 500);
    runtime
        .register_model(descriptor(
            1,
            ModelClass::Lrm,
            vec![QuantizedVariant {
                precision: Precision::Int4,
                footprint_mb: 4_000,
                expected_tokens_per_sec: 10.0,
            }],
        ))
        .unwrap();

    let contract = CapabilityContract {
        latency_budget_ms: 5_000,
        always_on: false,
    };
    assert!(runtime.select_variant(ModelClass::Lrm, &contract).is_none());
}

#[test]
fn tampered_descriptor_is_rejected_at_registration() {
    let runtime = LocalAiRuntime::new(Box::new(MockBackend), 8_000);
    let mut d = descriptor(
        1,
        ModelClass::Slm,
        vec![QuantizedVariant {
            precision: Precision::Fp16,
            footprint_mb: 1_000,
            expected_tokens_per_sec: 50.0,
        }],
    );
    d.checksum ^= 0xdead_beef;
    let result = runtime.register_model(d);
    assert!(matches!(
        result,
        Err(hyperion_ai_runtime::RuntimeError::IntegrityFailure)
    ));
}
