//! docs/17 T8: real Ed25519 signature verification and canary-score-drift
//! gating block a model promotion, they don't just log a warning.

use hyperion_ai_runtime::{sign, ModelClass, ModelDescriptor, Precision, QuantizedVariant};
use hyperion_crypto::Keystore;
use hyperion_security::{canary_gate_model_promotion, CanaryResult, PromotionStatus};

fn descriptor(model_id: u64, keystore: &Keystore) -> ModelDescriptor {
    let mut d = ModelDescriptor {
        model_id,
        class: ModelClass::Slm,
        variants: vec![QuantizedVariant {
            precision: Precision::Int8,
            footprint_mb: 512,
            expected_tokens_per_sec: 20.0,
        }],
        signature: None,
    };
    d.signature = Some(sign(&d, keystore));
    d
}

#[test]
fn a_low_drift_candidate_with_a_valid_signature_is_promoted() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    let candidate = descriptor(1, &keystore);
    let record =
        canary_gate_model_promotion(&candidate, 0.91, 0.90, 0.05, &keystore.verifying_key());
    assert!(record.signature_verified);
    assert_eq!(record.canary_result, CanaryResult::Pass);
    assert_eq!(record.promotion_status, PromotionStatus::Promoted);
}

#[test]
fn a_high_drift_candidate_is_blocked_even_with_a_valid_signature() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    let candidate = descriptor(1, &keystore);
    let record =
        canary_gate_model_promotion(&candidate, 0.40, 0.90, 0.05, &keystore.verifying_key());
    assert!(record.signature_verified);
    assert!(matches!(record.canary_result, CanaryResult::Fail { .. }));
    assert_eq!(record.promotion_status, PromotionStatus::Blocked);
}

#[test]
fn a_tampered_artifact_is_blocked_regardless_of_its_canary_score() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    let mut candidate = descriptor(1, &keystore);
    candidate.variants[0].footprint_mb += 1; // tampered after signing
    let record =
        canary_gate_model_promotion(&candidate, 0.90, 0.90, 0.05, &keystore.verifying_key());
    assert!(!record.signature_verified);
    assert_eq!(record.canary_result, CanaryResult::IntegrityMismatch);
    assert_eq!(record.promotion_status, PromotionStatus::Blocked);
}

#[test]
fn a_forged_artifact_signed_by_a_different_key_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let real_signer = Keystore::open_or_create(&dir.path().join("real.key")).unwrap();
    let forger = Keystore::open_or_create(&dir.path().join("forger.key")).unwrap();

    let forged = descriptor(1, &forger);
    let record =
        canary_gate_model_promotion(&forged, 0.91, 0.90, 0.05, &real_signer.verifying_key());
    assert!(
        !record.signature_verified,
        "a descriptor signed by a forger's own real keypair must not verify against the real \
         signer's public key -- this is exactly what a non-cryptographic checksum could not \
         have caught, since a forger can always recompute a checksum"
    );
    assert_eq!(record.promotion_status, PromotionStatus::Blocked);
}
