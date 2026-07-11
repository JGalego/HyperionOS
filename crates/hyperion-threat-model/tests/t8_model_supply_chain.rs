//! docs/17 T8: model supply-chain compromise — a real Ed25519 signature
//! check blocks a tampered artifact regardless of score, and a canary
//! differential test blocks promotion on score drift.

use hyperion_ai_runtime::{sign, ModelClass, ModelDescriptor, Precision, QuantizedVariant};
use hyperion_crypto::Keystore;
use hyperion_security::{canary_gate_model_promotion, PromotionStatus};

fn signed_descriptor(model_id: u64, keystore: &Keystore) -> ModelDescriptor {
    let mut d = ModelDescriptor {
        model_id,
        class: ModelClass::Slm,
        variants: vec![QuantizedVariant {
            precision: Precision::Int8,
            footprint_mb: 256,
            expected_tokens_per_sec: 30.0,
        }],
        signature: None,
    };
    d.signature = Some(sign(&d, keystore));
    d
}

#[test]
fn t8_a_tampered_model_artifact_is_blocked_regardless_of_its_canary_score() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    let mut poisoned = signed_descriptor(1, &keystore);
    poisoned.variants[0].footprint_mb += 1; // tampered after the real signature was computed
    let record =
        canary_gate_model_promotion(&poisoned, 0.95, 0.90, 0.10, &keystore.verifying_key());

    assert!(!record.signature_verified);
    assert_eq!(
        record.promotion_status,
        PromotionStatus::Blocked,
        "a poisoned artifact must never be promoted, even with a flattering canary score"
    );
}

#[test]
fn t8_a_legitimate_low_drift_update_is_promoted() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    let candidate = signed_descriptor(1, &keystore);
    let record =
        canary_gate_model_promotion(&candidate, 0.91, 0.90, 0.05, &keystore.verifying_key());

    assert!(record.signature_verified);
    assert_eq!(record.promotion_status, PromotionStatus::Promoted);
}

#[test]
fn t8_an_artifact_signed_by_an_untrusted_key_is_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let real_signer = Keystore::open_or_create(&dir.path().join("real.key")).unwrap();
    let attacker = Keystore::open_or_create(&dir.path().join("attacker.key")).unwrap();

    let forged = signed_descriptor(1, &attacker);
    let record =
        canary_gate_model_promotion(&forged, 0.95, 0.90, 0.10, &real_signer.verifying_key());

    assert!(
        !record.signature_verified,
        "a supply-chain attacker who controls their own real keypair, but not the trusted \
         device key, must still be rejected -- unlike a checksum, which they could recompute \
         over tampered content and pass regardless"
    );
    assert_eq!(record.promotion_status, PromotionStatus::Blocked);
}
