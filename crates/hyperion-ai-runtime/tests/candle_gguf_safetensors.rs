//! This crate's own previously-named "real GGUF/safetensors model loading" gap, proven for real:
//! `CandleBackend::load_gguf`/`CandleBackend::load_safetensors` each run a real forward pass
//! through real weights loaded from the two real interchange formats Hugging Face Hub actually
//! ships checkpoints in -- neither of which is Karpathy's own bespoke binary layout
//! `CandleBackend::load` already proved.
//!
//! `#[cfg(feature = "candle")]`-gated like the backend itself: these tests download real files
//! from the Hugging Face Hub on first run (cached by `hf-hub` afterward) -- a real ~1.2 MB
//! quantized GGUF checkpoint, and a real ~60 MB safetensors export -- so they deliberately do not
//! run as part of the default `cargo test --workspace` gate. Invoke explicitly with
//! `cargo test -p hyperion-ai-runtime --features candle --test candle_gguf_safetensors`.

#![cfg(feature = "candle")]

use hyperion_ai_runtime::{
    sign, CandleBackend, CapabilityContract, InferenceBackend, InferenceRequest, LocalAiRuntime,
    ModelClass, ModelDescriptor, Precision, QuantizedVariant,
};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;

fn setup(
    backend: Box<dyn InferenceBackend>,
) -> (
    LocalAiRuntime,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    CapabilityContract,
) {
    let runtime = LocalAiRuntime::new(backend, 8_000);

    let key_dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&key_dir.path().join("device.key")).unwrap();

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
    runtime
        .register_model(descriptor, &keystore.verifying_key())
        .expect("register the real, really-signed model descriptor");

    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let contract = CapabilityContract {
        latency_budget_ms: 60_000,
        always_on: false,
    };
    (runtime, monitor, token, contract)
}

#[test]
fn a_real_quantized_gguf_checkpoint_runs_a_real_forward_pass() {
    let backend = CandleBackend::load_gguf_default().expect(
        "download (or reuse a cached) real quantized GGUF checkpoint + tokenizer.json and load \
         real weights",
    );
    let (runtime, monitor, token, contract) = setup(Box::new(backend));
    let request = InferenceRequest {
        prompt: "Once upon a time".to_string(),
    };

    let result = runtime
        .infer(&monitor, &token, ModelClass::Slm, &contract, &request)
        .expect("a real, registered, resident Slm model must really run inference");

    assert!(
        !result.text.trim().is_empty(),
        "a real forward pass through a real quantized GGUF model must produce real, non-empty \
         generated text"
    );
    assert_ne!(
        result.text, request.prompt,
        "real generation must produce genuinely new text, not just echo the prompt back"
    );
}

#[test]
fn a_real_safetensors_checkpoint_runs_a_real_forward_pass() {
    let backend = CandleBackend::load_safetensors_default().expect(
        "download (or reuse a cached) real safetensors checkpoint + tokenizer.json and load \
         real weights",
    );
    let (runtime, monitor, token, contract) = setup(Box::new(backend));
    let request = InferenceRequest {
        prompt: "Once upon a time".to_string(),
    };

    let result = runtime
        .infer(&monitor, &token, ModelClass::Slm, &contract, &request)
        .expect("a real, registered, resident Slm model must really run inference");

    assert!(
        !result.text.trim().is_empty(),
        "a real forward pass through a real safetensors model must produce real, non-empty \
         generated text"
    );
    assert_ne!(
        result.text, request.prompt,
        "real generation must produce genuinely new text, not just echo the prompt back"
    );
}
