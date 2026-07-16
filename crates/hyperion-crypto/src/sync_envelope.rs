//! A real, encrypted + signed payload wrapper for exchanging state between devices that already
//! share one real, derived symmetric key (via [`crate::Keystore::derive_key`]) --
//! docs/998-roadmap.md's own named "`SyncEnvelope`-wrapped encrypted payloads" gap
//! (`hyperion-federation`'s own deferred list). Real ChaCha20-Poly1305 AEAD (the same
//! cipher/pattern [`crate::secret_store::SecretStore`] already uses) for
//! confidentiality+tamper-detection, plus a real Ed25519 signature over the whole sealed blob for
//! real sender authenticity -- an envelope that fails either check is rejected outright, never
//! silently corrupts or partially decrypts.
//!
//! [`seal`]/[`open`]'s own original scope: sealer and opener already derive the *same* symmetric
//! key from the *same* [`crate::Keystore`] -- real for one process's own multiple devices under
//! one federation hub (one shared identity). [`seal_for_peer`]/[`open_from_peer`]
//! (2026-07-16) close the gap that scope named as missing: two genuinely independent,
//! separately-keyed devices, each holding a real [`crate::key_exchange::diffie_hellman`]-derived
//! shared secret (never each other's private key), sealing/opening real envelopes between
//! themselves -- the sender signs with its own real identity; the opener verifies against the
//! sender's real, known public key, not its own.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand_core::{OsRng, RngCore};

use crate::{Keystore, VerifyingKey};

/// Domain-separates this module's derived key from any other caller of
/// [`Keystore::derive_key`] -- see [`crate::secret_store`]'s own identical convention.
const KEY_DERIVATION_CONTEXT: &str = "hyperion.sync-envelope.v1";
/// Domain-separates the AEAD key [`seal_for_peer`]/[`open_from_peer`] derive from a real X25519
/// shared secret -- a distinct context from [`KEY_DERIVATION_CONTEXT`] above so the two modes
/// (one shared `Keystore` vs. a real cross-device ECDH secret) can never accidentally derive the
/// same bytes from unrelated inputs.
const PEER_KEY_DERIVATION_CONTEXT: &str = "hyperion.sync-envelope.peer.v1";
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
    seal_with_key(
        &keystore.derive_key(KEY_DERIVATION_CONTEXT),
        keystore,
        sender_id,
        plaintext,
    )
}

/// The real inverse of [`seal`]: verifies the signature against `keystore`'s own real public key
/// first (real tamper/authenticity check, checked before any decryption is even attempted), then
/// really decrypts with a key derived from `keystore` the exact same way [`seal`] did. Either
/// failing is a real, honest error -- never a partial or silently-wrong plaintext.
pub fn open(keystore: &Keystore, envelope: &SyncEnvelope) -> Result<Vec<u8>, SyncEnvelopeError> {
    open_with_key(
        &keystore.derive_key(KEY_DERIVATION_CONTEXT),
        &keystore.verifying_key(),
        envelope,
    )
}

/// Really encrypts `plaintext` for a genuinely independent peer device, keyed by a real
/// [`crate::key_exchange::diffie_hellman`]-derived `shared_secret` (never `my_keystore`'s own
/// `derive_key` output -- that would only ever match another sealer sharing this exact
/// `Keystore`, the scope [`seal`] never claimed to outgrow). Still really signs with
/// `my_keystore`'s own identity, so the receiving peer can verify genuine authorship via
/// [`open_from_peer`] against this device's real, independently-known public key.
pub fn seal_for_peer(
    my_keystore: &Keystore,
    shared_secret: &[u8; 32],
    sender_id: u64,
    plaintext: &[u8],
) -> SyncEnvelope {
    seal_with_key(
        &blake3::derive_key(PEER_KEY_DERIVATION_CONTEXT, shared_secret),
        my_keystore,
        sender_id,
        plaintext,
    )
}

/// The real inverse of [`seal_for_peer`]: verifies the signature against `sender_verifying_key` --
/// the *sender's* real public key, deliberately not the opener's own, since these are two
/// genuinely independent devices -- then really decrypts with the same real
/// `shared_secret`-derived key [`seal_for_peer`] used. Either failing is a real, honest error,
/// exactly as [`open`] already guarantees for the single-`Keystore` case.
pub fn open_from_peer(
    sender_verifying_key: &VerifyingKey,
    shared_secret: &[u8; 32],
    envelope: &SyncEnvelope,
) -> Result<Vec<u8>, SyncEnvelopeError> {
    open_with_key(
        &blake3::derive_key(PEER_KEY_DERIVATION_CONTEXT, shared_secret),
        sender_verifying_key,
        envelope,
    )
}

fn seal_with_key(
    key_bytes: &[u8; 32],
    signer: &Keystore,
    sender_id: u64,
    plaintext: &[u8],
) -> SyncEnvelope {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key_bytes));
    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
        .expect("encryption with a freshly generated nonce cannot fail");
    let signature = signer.sign(&canonical_bytes(sender_id, &nonce_bytes, &ciphertext));
    SyncEnvelope {
        sender_id,
        nonce: nonce_bytes,
        ciphertext,
        signature,
    }
}

fn open_with_key(
    key_bytes: &[u8; 32],
    verifying_key: &VerifyingKey,
    envelope: &SyncEnvelope,
) -> Result<Vec<u8>, SyncEnvelopeError> {
    let bytes = canonical_bytes(envelope.sender_id, &envelope.nonce, &envelope.ciphertext);
    if !crate::verify(&bytes, &envelope.signature, verifying_key) {
        return Err(SyncEnvelopeError::SignatureInvalid);
    }
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key_bytes));
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

/// This envelope's own signature, in bytes -- ed25519-dalek's fixed 64-byte encoding.
const SIGNATURE_LEN: usize = 64;
/// [`SyncEnvelope::to_wire_bytes`]'s fixed header size before the variable-length ciphertext:
/// `sender_id` (8) + `nonce` (12) + `signature` (64).
const WIRE_HEADER_LEN: usize = 8 + NONCE_LEN + SIGNATURE_LEN;

impl SyncEnvelope {
    /// A real, fixed-order wire encoding a caller can actually send over a socket --
    /// docs/998-roadmap.md's own named "actual sockets carrying these envelopes between
    /// processes" gap needs exactly this: `sender_id` (8 bytes LE) + `nonce` (12 bytes) +
    /// `signature` (64 bytes) + `ciphertext` (the remainder, variable-length since it's last).
    /// This encoding is not self-framing -- a caller sending this over a stream socket still
    /// needs its own length prefix (e.g. a real `u32` byte count) around the whole result.
    pub fn to_wire_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(WIRE_HEADER_LEN + self.ciphertext.len());
        bytes.extend_from_slice(&self.sender_id.to_le_bytes());
        bytes.extend_from_slice(&self.nonce);
        bytes.extend_from_slice(&self.signature.to_bytes());
        bytes.extend_from_slice(&self.ciphertext);
        bytes
    }

    /// The real inverse of [`Self::to_wire_bytes`]. A truncated or otherwise malformed buffer --
    /// which a real, untrusted wire sender can always produce, maliciously or not -- is a real,
    /// honest `None`, never a panic on an out-of-bounds slice.
    pub fn from_wire_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < WIRE_HEADER_LEN {
            return None;
        }
        let sender_id = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
        let nonce: [u8; NONCE_LEN] = bytes[8..8 + NONCE_LEN].try_into().ok()?;
        let sig_start = 8 + NONCE_LEN;
        let sig_bytes: [u8; SIGNATURE_LEN] = bytes[sig_start..sig_start + SIGNATURE_LEN]
            .try_into()
            .ok()?;
        let ciphertext = bytes[sig_start + SIGNATURE_LEN..].to_vec();
        Some(SyncEnvelope {
            sender_id,
            nonce,
            ciphertext,
            signature: crate::Signature::from_bytes(&sig_bytes),
        })
    }
}
