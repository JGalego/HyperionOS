//! docs/998-roadmap.md's own named "SyncEnvelope-wrapped encrypted payloads" gap: a real
//! ChaCha20-Poly1305-sealed, Ed25519-signed envelope round-trips its real plaintext, and rejects
//! (never silently corrupts) a tampered ciphertext, a tampered signature, or the wrong key.

use hyperion_crypto::sync_envelope::{open, open_from_peer, seal, seal_for_peer};
use hyperion_crypto::{diffie_hellman, Keystore, SyncEnvelope, SyncEnvelopeError};

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
fn two_genuinely_independent_devices_seal_and_open_for_each_other_via_a_real_x25519_exchange() {
    let alice = Keystore::ephemeral();
    let bob = Keystore::ephemeral();

    // Real X25519 key agreement -- each side independently derives the same shared secret from
    // its own private half and the other's real public key, never from a shared Keystore.
    let alice_shared = diffie_hellman(&alice, &bob.x25519_public());
    let bob_shared = diffie_hellman(&bob, &alice.x25519_public());
    assert_eq!(alice_shared, bob_shared);

    let envelope = seal_for_peer(&alice, &alice_shared, 1, b"a real cross-device message");
    let opened = open_from_peer(&alice.verifying_key(), &bob_shared, &envelope)
        .expect("bob's independently-derived shared secret must open alice's real envelope");
    assert_eq!(opened, b"a real cross-device message");
}

#[test]
fn open_from_peer_rejects_the_wrong_senders_verifying_key() {
    let alice = Keystore::ephemeral();
    let bob = Keystore::ephemeral();
    let impostor = Keystore::ephemeral();

    let shared = diffie_hellman(&alice, &bob.x25519_public());
    let envelope = seal_for_peer(&alice, &shared, 1, b"really from alice");

    let result = open_from_peer(&impostor.verifying_key(), &shared, &envelope);
    assert!(
        matches!(result, Err(SyncEnvelopeError::SignatureInvalid)),
        "got: {result:?}"
    );
}

#[test]
fn open_from_peer_rejects_a_shared_secret_from_a_different_pair_of_devices() {
    let alice = Keystore::ephemeral();
    let bob = Keystore::ephemeral();
    let mallory = Keystore::ephemeral();

    let alice_bob_shared = diffie_hellman(&alice, &bob.x25519_public());
    let alice_mallory_shared = diffie_hellman(&alice, &mallory.x25519_public());

    let envelope = seal_for_peer(&alice, &alice_bob_shared, 1, b"only for bob");
    let result = open_from_peer(&alice.verifying_key(), &alice_mallory_shared, &envelope);
    assert!(
        matches!(result, Err(SyncEnvelopeError::DecryptionFailed)),
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

#[test]
fn a_sealed_envelope_round_trips_through_its_real_wire_encoding() {
    let keystore = Keystore::ephemeral();
    let envelope = seal(&keystore, 3, b"a real payload bound for a real socket");

    let wire = envelope.to_wire_bytes();
    let decoded = SyncEnvelope::from_wire_bytes(&wire)
        .expect("a real envelope's own wire encoding must always decode");

    let opened = open(&keystore, &decoded).expect("the wire-decoded envelope must still open");
    assert_eq!(opened, b"a real payload bound for a real socket");
    assert_eq!(decoded.sender_id, 3);
}

#[test]
fn a_buffer_shorter_than_the_fixed_wire_header_is_rejected_rather_than_panicking() {
    let keystore = Keystore::ephemeral();
    let envelope = seal(&keystore, 1, b"real payload");
    let wire = envelope.to_wire_bytes();
    // Shorter than sender_id(8) + nonce(12) + signature(64) -- too short to even contain a
    // complete header, regardless of how much (if any) ciphertext follows.
    let header_len = 8 + 12 + 64;

    assert!(
        SyncEnvelope::from_wire_bytes(&wire[..header_len - 1]).is_none(),
        "a buffer shorter than the fixed header must be rejected"
    );
    assert!(
        SyncEnvelope::from_wire_bytes(&[]).is_none(),
        "an empty buffer must be rejected"
    );
}

#[test]
fn a_wire_buffer_truncated_within_the_ciphertext_decodes_but_fails_to_open() {
    let keystore = Keystore::ephemeral();
    let envelope = seal(&keystore, 1, b"real payload");
    let wire = envelope.to_wire_bytes();

    // Long enough to contain a full header plus a shortened ciphertext -- this still parses (the
    // wire encoding has no separate ciphertext-length field), but the corrupted ciphertext must
    // fail to decrypt rather than silently produce wrong plaintext.
    let truncated = &wire[..wire.len() - 1];
    let decoded =
        SyncEnvelope::from_wire_bytes(truncated).expect("still long enough to contain a header");
    assert!(open(&keystore, &decoded).is_err());
}
