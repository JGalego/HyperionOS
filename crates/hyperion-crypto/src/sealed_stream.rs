//! A real, generic AEAD seal/open primitive over a raw 32-byte key -- the same
//! `ChaCha20Poly1305` + random-nonce-per-message pattern [`crate::secret_store::SecretStore`]
//! established for one whole-file blob, generalized here for a caller that needs to seal many
//! independent, individually-decryptable messages -- `hyperion-storage`'s own per-record
//! Write-Ahead Log encryption at rest, most concretely, where re-sealing an entire growing file
//! on every append would defeat the point of an append-only log.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand_core::{OsRng, RngCore};

const NONCE_LEN: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum SealError {
    #[error("sealed data is too short to hold a real nonce + ciphertext")]
    Truncated,
    #[error("decryption failed -- wrong key or corrupt/tampered data")]
    DecryptionFailed,
}

/// A real ChaCha20-Poly1305 key, ready to seal/open any number of independent messages --
/// typically derived via [`crate::Keystore::derive_key`] so no new passphrase or key-management
/// UX is needed.
pub struct SealingKey {
    cipher: ChaCha20Poly1305,
}

impl SealingKey {
    /// Wraps an already-derived 32-byte key (e.g. from [`crate::Keystore::derive_key`]) for
    /// sealing/opening.
    pub fn from_bytes(key_bytes: [u8; 32]) -> Self {
        SealingKey {
            cipher: ChaCha20Poly1305::new(Key::from_slice(&key_bytes)),
        }
    }

    /// Seals `plaintext` under a fresh random nonce -- returns `[12-byte nonce][ciphertext]`,
    /// independently decryptable by [`Self::open`] without any other message's nonce or
    /// ciphertext.
    pub fn seal(&self, plaintext: &[u8]) -> Vec<u8> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
            .expect("sealing with a real, correctly-sized key never fails");
        let mut sealed = nonce_bytes.to_vec();
        sealed.extend_from_slice(&ciphertext);
        sealed
    }

    /// Opens data produced by [`Self::seal`] under the same key -- a real authentication failure
    /// (wrong key, corrupt data, or a truncated/tampered ciphertext) is `Err`, never silently
    /// wrong plaintext.
    pub fn open(&self, sealed: &[u8]) -> Result<Vec<u8>, SealError> {
        if sealed.len() < NONCE_LEN {
            return Err(SealError::Truncated);
        }
        let (nonce_bytes, ciphertext) = sealed.split_at(NONCE_LEN);
        self.cipher
            .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
            .map_err(|_| SealError::DecryptionFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_then_open_round_trips_exactly() {
        let key = SealingKey::from_bytes([7u8; 32]);
        let sealed = key.seal(b"real plaintext");
        assert_eq!(key.open(&sealed).unwrap(), b"real plaintext");
    }

    #[test]
    fn two_seals_of_the_same_plaintext_never_produce_identical_bytes() {
        let key = SealingKey::from_bytes([7u8; 32]);
        let a = key.seal(b"same plaintext");
        let b = key.seal(b"same plaintext");
        assert_ne!(a, b, "a fresh random nonce must make every seal unique");
    }

    #[test]
    fn opening_with_the_wrong_key_fails_closed() {
        let key_a = SealingKey::from_bytes([1u8; 32]);
        let key_b = SealingKey::from_bytes([2u8; 32]);
        let sealed = key_a.seal(b"secret");
        assert!(matches!(
            key_b.open(&sealed),
            Err(SealError::DecryptionFailed)
        ));
    }

    #[test]
    fn opening_truncated_data_fails_closed() {
        let key = SealingKey::from_bytes([3u8; 32]);
        assert!(matches!(key.open(&[0u8; 4]), Err(SealError::Truncated)));
    }

    #[test]
    fn opening_tampered_ciphertext_fails_closed() {
        let key = SealingKey::from_bytes([4u8; 32]);
        let mut sealed = key.seal(b"secret");
        let last = sealed.len() - 1;
        sealed[last] ^= 0xff;
        assert!(matches!(
            key.open(&sealed),
            Err(SealError::DecryptionFailed)
        ));
    }
}
