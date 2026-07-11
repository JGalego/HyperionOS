//! PRODUCTION_BOOT_PROMPT.md M8's real deliverable, proven for real: a real Candle backend runs a
//! real forward pass through a real, downloaded model's real weights, producing real generated
//! text -- not `MockBackend`'s deterministic echo.
//!
//! `#[cfg(feature = "candle")]`-gated like the backend itself: this test downloads a real ~61 MB
//! model file (and a small tokenizer) from the Hugging Face Hub on first run (cached by `hf-hub`
//! afterward), so it deliberately does not run as part of the default `cargo test --workspace`
//! gate (which must stay network-free and fast) -- invoke explicitly with
//! `cargo test -p hyperion-ai-runtime --features candle --test candle_inference`.

#![cfg(feature = "candle")]

use hyperion_ai_runtime::{
    sign, CandleBackend, CapabilityContract, InferenceRequest, LocalAiRuntime, ModelClass,
    ModelDescriptor, Precision, QuantizedVariant,
};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;

#[test]
fn a_real_candle_backend_runs_real_inference_through_a_real_downloaded_model() {
    let backend = CandleBackend::load().expect(
        "download (or reuse a cached) real stories15M.bin + tokenizer.json and load real weights",
    );
    let runtime = LocalAiRuntime::new(Box::new(backend), 8_000);

    let key_dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&key_dir.path().join("device.key")).unwrap();

    let mut descriptor = ModelDescriptor {
        model_id: 1,
        class: ModelClass::Slm,
        variants: vec![QuantizedVariant {
            precision: Precision::Fp16,
            // stories15M.bin is ~61 MB on disk; declared generously above that so real-model
            // residency/fit logic has no reason to reject it.
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
    let request = InferenceRequest {
        prompt: "Once upon a time".to_string(),
    };

    let result = runtime
        .infer(&monitor, &token, ModelClass::Slm, &contract, &request)
        .expect("a real, registered, resident Slm model must really run inference");

    assert!(
        !result.text.trim().is_empty(),
        "a real forward pass through a real model must produce real, non-empty generated text"
    );
    assert_ne!(
        result.text, request.prompt,
        "real generation must produce genuinely new text, not just echo the prompt back \
         (that would indicate MockBackend's shape, not a real forward pass)"
    );
}
