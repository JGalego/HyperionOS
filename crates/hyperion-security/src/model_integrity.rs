use hyperion_ai_runtime::{checksum, ModelDescriptor};

use crate::types::{CanaryResult, ModelIntegrityRecord, PromotionStatus};

/// docs/17 T8's mitigation: content-addressed + signature-verified model
/// artifacts (here, `hyperion-ai-runtime`'s existing non-cryptographic
/// checksum standing in for a real signature — see this crate's doc
/// comment) and a canary differential test blocking promotion on score
/// drift, both gates evaluated *before* a candidate model can replace the
/// baseline the Risk Assessment Engine's own judgment ultimately runs on
/// top of.
pub fn canary_gate_model_promotion(
    candidate: &ModelDescriptor,
    candidate_score: f32,
    baseline_score: f32,
    max_drift: f32,
) -> ModelIntegrityRecord {
    let checksum_verified = checksum(candidate) == candidate.checksum;
    if !checksum_verified {
        return ModelIntegrityRecord {
            model_id: candidate.model_id,
            checksum_verified,
            canary_result: CanaryResult::IntegrityMismatch,
            promotion_status: PromotionStatus::Blocked,
        };
    }

    let drift = (baseline_score - candidate_score).abs();
    let canary_result = if drift > max_drift {
        CanaryResult::Fail {
            drift_millipoints: (drift * 1000.0) as u32,
        }
    } else {
        CanaryResult::Pass
    };
    let promotion_status = if canary_result == CanaryResult::Pass {
        PromotionStatus::Promoted
    } else {
        PromotionStatus::Blocked
    };

    ModelIntegrityRecord {
        model_id: candidate.model_id,
        checksum_verified,
        canary_result,
        promotion_status,
    }
}
