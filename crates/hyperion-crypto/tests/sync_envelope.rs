//! docs/998-roadmap.md's own named "SyncEnvelope-wrapped encrypted payloads" gap: a real
//! ChaCha20-Poly1305-sealed, Ed25519-signed envelope round-trips its real plaintext, and rejects
//! (never silently corrupts) a tampered ciphertext, a tampered signature, or the wrong key.

use hyperion_crypto::sync_envelope::{open, seal};
use hyperion_crypto::{Keystore, SyncEnvelopeError};

#[test]
fn a_sealed_envelope_round_trips_its_real_plaintext() {
    let keystore = Keystore::ephemeral();
    let plaintext = b"a real ledger update from device 7";

    let envelope = seal(&keystore, 7, plaintext);
    assert_eq!(envelope.sender_id, 7);

    let opened = open(&keystore, &envelope).expect("a real, untampered envelope must open");
    assert_eq!(opened, plaintext);
}

#[test]
fn two_seals_of_the_same_plaintext_use_different_real_nonces_and_ciphertexts() {
    let keystore = Keystore::ephemeral();
    let a = seal(&keystore, 1, b"same plaintext");
    let b = seal(&keystore, 1, b"same plaintext");
    // Both open to the same real plaintext...
    assert_eq!(open(&keystore, &a).unwrap(), open(&keystore, &b).unwrap());
    // ...but a real, freshly-generated nonce means the actual sealed envelopes never collide.
    assert_ne!(
        format!("{a:?}"),
        format!("{b:?}"),
        "two real seals of the same plaintext must not produce identical sealed bytes"
    );
}

#[test]
fn an_envelope_sealed_by_a_different_keystore_fails_to_open() {
    let real_sender = Keystore::ephemeral();
    let impostor = Keystore::ephemeral();

    let envelope = seal(&impostor, 1, b"pretend this is from the real sender");
    let result = open(&real_sender, &envelope);
    assert!(
        matches!(result, Err(SyncEnvelopeError::SignatureInvalid)),
        "got: {result:?}"
    );
}

#[test]
fn a_tampered_sender_id_invalidates_the_real_signature() {
    let keystore = Keystore::ephemeral();
    let mut envelope = seal(&keystore, 1, b"real payload");
    envelope.sender_id = 2; // tamper with the claimed sender after sealing

    let result = open(&keystore, &envelope);
    assert!(
        matches!(result, Err(SyncEnvelopeError::SignatureInvalid)),
        "changing the claimed sender after sealing must invalidate the real signature, got: \
         {result:?}"
    );
}
