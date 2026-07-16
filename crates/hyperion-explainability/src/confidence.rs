use std::collections::HashMap;

use hyperion_ai_runtime::{CapabilityContract, InferenceRequest, LocalAiRuntime, ModelClass};
use hyperion_capability::{CapabilityMonitor, CapabilityToken};

use crate::types::{ConfidenceMethod, ConfidenceScore};

/// Matches `hyperion-context::ContextEngine::summarize`'s own resident-`Slm`-class latency
/// budget: generous enough that a real, modest-throughput resident variant still passes tier
/// selection, without letting a `samples`-call round stall indefinitely.
const SELF_CONSISTENCY_LATENCY_BUDGET_MS: u64 = 15_000;

/// docs/18-explainability-and-trust.md's own named "`ConfidenceScore.method` implementations"
/// gap, closed for `ConfidenceMethod::SelfConsistency` — docs/18 §9's own "self-consistency
/// across repeated sampling": calls `ai_runtime.infer` with the identical `prompt` `samples`
/// real times, and returns the real majority-answer agreement fraction as the confidence value.
/// `Verifier`/`Ensemble` remain separately deferred (see this crate's own doc comment):
/// `Verifier` needs real formal verification this workspace doesn't have, and `Ensemble` needs
/// [23 — Multi-Model Orchestration](../23-multi-model-orchestration.md)'s actual candidate
/// models, neither of which this function touches.
///
/// `None` if this token isn't authorized for real inference, nothing is resident locally for
/// `ModelClass::Slm`, or any one of the `samples` real calls fails for any other reason — a
/// caller falls back to its own existing confidence estimate (e.g. a plain `Heuristic`-tagged
/// score), the same graceful-degradation contract every other `ai_runtime`-backed method in this
/// workspace already uses; this never fabricates a partial score from fewer than `samples` real
/// completions. `samples` must be at least 1 — with none, there is nothing to be self-consistent
/// over.
pub fn self_consistency_confidence(
    ai_runtime: &LocalAiRuntime,
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    prompt: &str,
    samples: usize,
) -> Option<ConfidenceScore> {
    if samples == 0 {
        return None;
    }

    let contract = CapabilityContract {
        latency_budget_ms: SELF_CONSISTENCY_LATENCY_BUDGET_MS,
        always_on: false,
    };
    let request = InferenceRequest {
        prompt: prompt.to_string(),
    };

    let mut outputs = Vec::with_capacity(samples);
    for _ in 0..samples {
        let result = ai_runtime
            .infer(monitor, token, ModelClass::Slm, &contract, &request)
            .ok()?;
        outputs.push(result.text);
    }

    let mut counts: HashMap<&str, usize> = HashMap::new();
    for output in &outputs {
        *counts.entry(output.as_str()).or_insert(0) += 1;
    }
    let majority = counts.values().copied().max().unwrap_or(0);
    let agreement = majority as f32 / outputs.len() as f32;

    Some(ConfidenceScore {
        value: agreement,
        method: ConfidenceMethod::SelfConsistency,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use hyperion_ai_runtime::{
        sign, CancellationToken, InferenceBackend, LocalAiRuntime, ModelDescriptor, Precision,
        QuantizedVariant,
    };
    use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
    use hyperion_crypto::Keystore;

    use super::*;

    /// A real `InferenceBackend` that cycles deterministically through `answers`, so a real
    /// multi-call self-consistency round can genuinely disagree with itself -- `MockBackend`'s
    /// own echo is identical every call for a fixed prompt/model_id, which can only ever prove
    /// the always-agrees case.
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

    fn monitor_and_token() -> (CapabilityMonitor, hyperion_capability::CapabilityToken) {
        let mut monitor = CapabilityMonitor::new();
        let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
        (monitor, token)
    }

    #[test]
    fn unanimous_real_answers_yield_full_confidence() {
        let (monitor, token) = monitor_and_token();
        let key_dir = tempfile::tempdir().unwrap();
        let keystore = Keystore::open_or_create(&key_dir.path().join("device.key")).unwrap();
        let ai_runtime = Arc::new(LocalAiRuntime::new(
            Box::new(CyclingBackend {
                answers: vec!["Lisbon"],
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
            self_consistency_confidence(&ai_runtime, &monitor, &token, "capital of Portugal?", 5)
                .unwrap();
        assert_eq!(score.value, 1.0);
        assert_eq!(score.method, ConfidenceMethod::SelfConsistency);
    }

    #[test]
    fn a_real_partial_disagreement_yields_a_real_fractional_confidence() {
        let (monitor, token) = monitor_and_token();
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
            self_consistency_confidence(&ai_runtime, &monitor, &token, "capital of Portugal?", 4)
                .unwrap();
        assert_eq!(
            score.value, 0.75,
            "3 of 4 real samples agreeing must yield exactly 0.75, not a fabricated value"
        );
    }

    #[test]
    fn no_model_registered_for_the_class_degrades_to_none_not_a_fabricated_score() {
        let (monitor, token) = monitor_and_token();
        // Deliberately no register_model call -- infer() must return InfeasibleLocally.
        let ai_runtime = Arc::new(LocalAiRuntime::new(
            Box::new(CyclingBackend {
                answers: vec!["Lisbon"],
                next: AtomicUsize::new(0),
            }),
            8_000,
        ));

        assert!(self_consistency_confidence(
            &ai_runtime,
            &monitor,
            &token,
            "capital of Portugal?",
            3
        )
        .is_none());
    }

    #[test]
    fn zero_samples_is_never_a_real_computation() {
        let (monitor, token) = monitor_and_token();
        let ai_runtime = Arc::new(LocalAiRuntime::new(
            Box::new(CyclingBackend {
                answers: vec!["Lisbon"],
                next: AtomicUsize::new(0),
            }),
            8_000,
        ));
        assert!(self_consistency_confidence(
            &ai_runtime,
            &monitor,
            &token,
            "capital of Portugal?",
            0
        )
        .is_none());
    }
}
