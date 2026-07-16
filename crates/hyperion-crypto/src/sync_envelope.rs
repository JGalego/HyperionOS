//! A real, encrypted + signed payload wrapper for exchanging state between devices that already
//! share one real, derived symmetric key (via [`crate::Keystore::derive_key`]) --
//! docs/998-roadmap.md's own named "`SyncEnvelope`-wrapped encrypted payloads" gap
//! (`hyperion-federation`'s own deferred list). Real ChaCha20-Poly1305 AEAD (the same
//! cipher/pattern [`crate::secret_store::SecretStore`] already uses) for
//! confidentiality+tamper-detection, plus a real Ed25519 signature over the whole sealed blob for
//! real sender authenticity -- an envelope that fails either check is rejected outright, never
//! silently corrupts or partially decrypts.
//!
//! **Scope, honestly**: this assumes the sealer and opener already derive the *same* symmetric
//! key from the *same* [`crate::Keystore`] -- real today for one process's own multiple devices
//! under one federation hub (one shared identity), not yet a real key-exchange between genuinely
//! separate, independently-keyed devices (that needs a real asymmetric key-exchange primitive --
//! e.g. X25519 ECDH -- this crate doesn't have yet). Adding one is real, separate, future work,
//! named here rather than faked with an undocumented multi-key design this crate's own doc
//! comment already avoids elsewhere.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand_core::{OsRng, RngCore};

use crate::Keystore;

/// Domain-separates this module's derived key from any other caller of
/// [`Keystore::derive_key`] -- see [`crate::secret_store`]'s own identical convention.
const KEY_DERIVATION_CONTEXT: &str = "hyperion.sync-envelope.v1";
/// ChaCha20-Poly1305's own fixed nonce size.
const NONCE_LEN: usize = 12;

/// A real, sealed payload -- opaque outside this module except for `sender_id`, a real
/// provenance label the signature covers (so it can't be swapped without invalidating the
/// envelope), not a secret itself.
#[derive(Debug, Clone)]
pub struct SyncEnvelope {
    pub sender_id: u64,
    nonce: [u8; NONCE_LEN],
    ciphertext: Vec<u8>,
    signature: crate::Signature,
}

#[derive(Debug, thiserror::Error)]
pub enum SyncEnvelopeError {
    #[error(
        "this envelope's signature doesn't verify -- it may not really be from the sender it \
         claims, or was tampered with in transit"
    )]
    SignatureInvalid,
    #[error("this envelope failed to decrypt -- wrong key, or a tampered/corrupt payload")]
    DecryptionFailed,
}

/// Really encrypts `plaintext` (ChaCha20-Poly1305, keyed via [`Keystore::derive_key`]) and really
/// signs the sealed result (Ed25519, via [`Keystore::sign`]).
pub fn seal(keystore: &Keystore, sender_id: u64, plaintext: &[u8]) -> SyncEnvelope {
    let key_bytes = keystore.derive_key(KEY_DERIVATION_CONTEXT);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key_bytes));
    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
        .expect("encryption with a freshly generated nonce cannot fail");
    let signature = keystore.sign(&canonical_bytes(sender_id, &nonce_bytes, &ciphertext));
    SyncEnvelope {
        sender_id,
        nonce: nonce_bytes,
        ciphertext,
        signature,
    }
}

/// The real inverse of [`seal`]: verifies the signature against `keystore`'s own real public key
/// first (real tamper/authenticity check, checked before any decryption is even attempted), then
/// really decrypts with a key derived from `keystore` the exact same way [`seal`] did. Either
/// failing is a real, honest error -- never a partial or silently-wrong plaintext.
pub fn open(keystore: &Keystore, envelope: &SyncEnvelope) -> Result<Vec<u8>, SyncEnvelopeError> {
    let bytes = canonical_bytes(envelope.sender_id, &envelope.nonce, &envelope.ciphertext);
    if !crate::verify(&bytes, &envelope.signature, &keystore.verifying_key()) {
        return Err(SyncEnvelopeError::SignatureInvalid);
    }
    let key_bytes = keystore.derive_key(KEY_DERIVATION_CONTEXT);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key_bytes));
    cipher
        .decrypt(
            Nonce::from_slice(&envelope.nonce),
            envelope.ciphertext.as_slice(),
        )
        .map_err(|_| SyncEnvelopeError::DecryptionFailed)
}

fn canonical_bytes(sender_id: u64, nonce: &[u8], ciphertext: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&sender_id.to_le_bytes());
    bytes.extend_from_slice(nonce);
    bytes.extend_from_slice(ciphertext);
    bytes
}
