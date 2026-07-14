//! A real, encrypted-at-rest secret store for arbitrary named secrets -- built for cloud
//! provider API keys (docs/998-roadmap.md "Phase 2: cloud providers"), but general enough
//! for any future caller needing the same "small number of named secrets, encrypted, persisted
//! to one file" shape.
//!
//! Deliberately a new, separate type from [`crate::Keystore`], not an extension of it:
//! `Keystore`'s on-disk format is a bare 32-byte Ed25519 seed with no room for a second value,
//! and it has no AEAD cipher dependency at all. This store's own symmetric key is *derived* from
//! the same per-device identity via [`crate::Keystore::derive_key`] -- no new passphrase or
//! key-management UX, and the raw Ed25519 seed itself never has to leave `Keystore`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand_core::{OsRng, RngCore};

use crate::Keystore;

/// Domain-separates this store's derived key from any other future caller of
/// [`Keystore::derive_key`] -- BLAKE3's own key-derivation contract is that distinct context
/// strings never produce colliding output, so this literal is this store's whole "namespace."
const KEY_DERIVATION_CONTEXT: &str = "hyperion.secret-store.v1";
/// ChaCha20-Poly1305's own fixed nonce size.
const NONCE_LEN: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum SecretStoreError {
    #[error("failed to read or write the real secret store file at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the secret store file at {0:?} is too short to hold a real nonce + ciphertext")]
    Truncated(PathBuf),
    #[error(
        "the secret store file at {0:?} failed to decrypt -- wrong device key or corrupt file"
    )]
    DecryptionFailed(PathBuf),
    #[error("the secret store file at {path:?} didn't hold valid JSON after decryption: {source}")]
    Corrupt {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

/// A real, encrypted-at-rest map of secret name -> secret value (e.g. `"openai"` -> a real API
/// key), persisted as one small file: `[12-byte random nonce][ChaCha20-Poly1305-sealed JSON
/// blob]`. Small and infrequently written, so re-encrypting the whole map on every [`Self::set`]
/// (rather than per-entry) keeps this simple with no real performance cost.
pub struct SecretStore {
    path: PathBuf,
    cipher: ChaCha20Poly1305,
    secrets: HashMap<String, String>,
}

impl SecretStore {
    /// Opens the real store at `path`, decrypting it with a key derived from `device_key`'s own
    /// identity, or starts a new, empty one if `path` doesn't exist yet. A *different*
    /// `device_key` than whichever one last wrote `path` fails closed with
    /// [`SecretStoreError::DecryptionFailed`] -- ChaCha20-Poly1305's real authentication tag
    /// check rejects it, never silently returning wrong or garbage secrets.
    pub fn open_or_create(path: &Path, device_key: &Keystore) -> Result<Self, SecretStoreError> {
        let key_bytes = device_key.derive_key(KEY_DERIVATION_CONTEXT);
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key_bytes));

        let secrets = if path.exists() {
            let blob = fs::read(path).map_err(|source| SecretStoreError::Io {
                path: path.to_path_buf(),
                source,
            })?;
            Self::decrypt(&cipher, &blob, path)?
        } else {
            HashMap::new()
        };

        Ok(SecretStore {
            path: path.to_path_buf(),
            cipher,
            secrets,
        })
    }

    fn decrypt(
        cipher: &ChaCha20Poly1305,
        blob: &[u8],
        path: &Path,
    ) -> Result<HashMap<String, String>, SecretStoreError> {
        if blob.len() < NONCE_LEN {
            return Err(SecretStoreError::Truncated(path.to_path_buf()));
        }
        let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
        let plaintext = cipher
            .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
            .map_err(|_| SecretStoreError::DecryptionFailed(path.to_path_buf()))?;
        serde_json::from_slice(&plaintext).map_err(|source| SecretStoreError::Corrupt {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Sets `provider`'s secret to `api_key` and immediately re-encrypts and persists the whole
    /// store -- a real, atomic (temp-file + rename) write, never a direct in-place write that
    /// could leave a torn file on a crash mid-write (the real gap [`crate::Keystore::
    /// persist_new_key`] itself still has, unfixed there deliberately -- a separate, existing
    /// file format this store doesn't touch).
    pub fn set(&mut self, provider: &str, api_key: &str) -> Result<(), SecretStoreError> {
        self.secrets
            .insert(provider.to_string(), api_key.to_string());
        self.persist()
    }

    /// This provider's real, decrypted secret, if one has ever been [`Self::set`].
    pub fn get(&self, provider: &str) -> Option<&str> {
        self.secrets.get(provider).map(String::as_str)
    }

    /// Every provider with a real secret already stored -- used to seed already-consented
    /// providers' capability grants at session startup without re-prompting for them.
    pub fn providers(&self) -> impl Iterator<Item = &str> {
        self.secrets.keys().map(String::as_str)
    }

    fn persist(&self) -> Result<(), SecretStoreError> {
        let plaintext =
            serde_json::to_vec(&self.secrets).expect("a HashMap<String, String> always serializes");

        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_slice())
            .expect("sealing with a real, correctly-sized key never fails");

        let mut blob = nonce_bytes.to_vec();
        blob.extend_from_slice(&ciphertext);

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|source| SecretStoreError::Io {
                path: self.path.clone(),
                source,
            })?;
        }
        let tmp_path = self.path.with_extension("tmp");
        fs::write(&tmp_path, &blob).map_err(|source| SecretStoreError::Io {
            path: self.path.clone(),
            source,
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600)).map_err(
                |source| SecretStoreError::Io {
                    path: self.path.clone(),
                    source,
                },
            )?;
        }
        fs::rename(&tmp_path, &self.path).map_err(|source| SecretStoreError::Io {
            path: self.path.clone(),
            source,
        })
    }
}
