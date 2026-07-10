//! docs/17 T8: content-hash verification and canary-score-drift gating
//! block a model promotion, they don't just log a warning.

use hyperion_ai_runtime::{checksum, ModelClass, ModelDescriptor, Precision, QuantizedVariant};
use hyperion_security::{canary_gate_model_promotion, CanaryResult, PromotionStatus};

fn descriptor(model_id: u64) -> ModelDescriptor {
    let mut d = ModelDescriptor {
        model_id,
        class: ModelClass::Slm,
        variants: vec![QuantizedVariant {
            precision: Precision::Int8,
            footprint_mb: 512,
            expected_tokens_per_sec: 20.0,
        }],
        checksum: 0,
    };
    d.checksum = checksum(&d);
    d
}

#[test]
fn a_low_drift_candidate_with_a_valid_checksum_is_promoted() {
    let candidate = descriptor(1);
    let record = canary_gate_model_promotion(&candidate, 0.91, 0.90, 0.05);
    assert!(record.checksum_verified);
    assert_eq!(record.canary_result, CanaryResult::Pass);
    assert_eq!(record.promotion_status, PromotionStatus::Promoted);
}

#[test]
fn a_high_drift_candidate_is_blocked_even_with_a_valid_checksum() {
    let candidate = descriptor(1);
    let record = canary_gate_model_promotion(&candidate, 0.40, 0.90, 0.05);
    assert!(record.checksum_verified);
    assert!(matches!(record.canary_result, CanaryResult::Fail { .. }));
    assert_eq!(record.promotion_status, PromotionStatus::Blocked);
}

#[test]
fn a_tampered_artifact_is_blocked_regardless_of_its_canary_score() {
    let mut candidate = descriptor(1);
    candidate.checksum = candidate.checksum.wrapping_add(1); // simulate tampering after signing
    let record = canary_gate_model_promotion(&candidate, 0.90, 0.90, 0.05);
    assert!(!record.checksum_verified);
    assert_eq!(record.canary_result, CanaryResult::IntegrityMismatch);
    assert_eq!(record.promotion_status, PromotionStatus::Blocked);
}
