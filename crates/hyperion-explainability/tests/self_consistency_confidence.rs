//! docs/18 §9's own "self-consistency across repeated sampling," proven end to end: a real
//! `self_consistency_confidence` score, computed against a real (test-double) `LocalAiRuntime`,
//! really flows into a real `ExplanationStore` record via `set_confidence` -- not just the pure
//! function in isolation (see `src/confidence.rs`'s own inline unit tests for that).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use hyperion_ai_runtime::{
    sign, CancellationToken, InferenceBackend, InferenceRequest, LocalAiRuntime, ModelClass,
    ModelDescriptor, Precision, QuantizedVariant,
};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_explainability::{
    self_consistency_confidence, ConfidenceMethod, Depth, ExplanationStore,
};

struct CyclingBackend {
    answers: Vec<&'static str>,
    next: AtomicUsize,
}

impl InferenceBackend for CyclingBackend {
    fn generate(
        &self,
        _model_id: u64,
        _request: &InferenceRequest,
        _cancel: &CancellationToken,
    ) -> String {
        let i = self.next.fetch_add(1, Ordering::Relaxed) % self.answers.len();
        self.answers[i].to_string()
    }
}

fn registered_slm_descriptor(keystore: &Keystore) -> ModelDescriptor {
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
    descriptor.signature = Some(sign(&descriptor, keystore));
    descriptor
}

#[test]
fn a_real_self_consistency_score_really_reaches_the_explanation_record() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let store = ExplanationStore::new();

    let key_dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&key_dir.path().join("device.key")).unwrap();
    let ai_runtime = Arc::new(LocalAiRuntime::new(
        Box::new(CyclingBackend {
            answers: vec!["Lisbon", "Lisbon", "Lisbon", "Porto"],
            next: AtomicUsize::new(0),
        }),
        8_000,
    ));
    ai_runtime
        .register_model(
            registered_slm_descriptor(&keystore),
            &keystore.verifying_key(),
        )
        .unwrap();

    let score =
        self_consistency_confidence(&ai_runtime, &monitor, &root, "capital of Portugal?", 4)
            .expect("a real, resident model must produce a real self-consistency score");
    assert_eq!(score.value, 0.75);
    assert_eq!(score.method, ConfidenceMethod::SelfConsistency);

    let action_id = 1;
    let id = store
        .begin(
            &monitor,
            &root,
            action_id,
            7,
            1,
            "geography.qa",
            vec![],
            1_000,
        )
        .unwrap();
    store
        .set_confidence(&monitor, &root, id, score, vec![])
        .unwrap();

    let view =
        hyperion_explainability::resolve_why(&store, &monitor, &root, action_id, Depth::Full)
            .unwrap()
            .expect("the record must really exist");
    let recorded = view
        .full
        .expect("Depth::Full must include the full record")
        .confidence
        .expect("the real self-consistency score must really be recorded");
    assert_eq!(recorded.value, 0.75);
    assert_eq!(recorded.method, ConfidenceMethod::SelfConsistency);
}
