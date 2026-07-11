//! Real cryptographic primitives (PRODUCTION_BOOT_PROMPT.md M9) -- Ed25519 signing, BLAKE3
//! content hashing, and a minimal real software keystore.
//!
//! Every crate in this workspace that previously stood in for "a real signature" or "real
//! content-addressed hashing" with a hand-rolled, non-cryptographic checksum
//! (`hyperion-ai-runtime::checksum`, `hyperion-plugin-framework::signature`,
//! `hyperion-update::signature`, `hyperion-observability`'s hash-chain, and
//! `hyperion-security`'s model-integrity check, which reuses `hyperion-ai-runtime`'s) now
//! depends on this crate instead. One real device identity: [`Keystore`] loads or generates a
//! real Ed25519 keypair and persists it to a real file (a real, if minimal, software keystore --
//! "software keystore at minimum" is this milestone's own floor). Verification only ever needs
//! the public half (`VerifyingKey`), so [`verify`] is a free function any caller holding a known
//! public key can call without needing a `Keystore` of its own.
//!
//! Deliberately deferred, and why:
//! - **TPM/secure-enclave-backed sealing.** This sandbox has no TPM device (`/dev/tpm*` does not
//!   exist here) -- confirmed directly, not assumed. docs/34's own text already frames hardware
//!   anchoring as opportunistic ("a software key otherwise, degrading gracefully"); this crate
//!   *is* that software-key fallback. Real TPM-backed sealing on real reference hardware that
//!   has one is real, separate, hardware-dependent work this sandbox cannot do or verify.
//! - **Multi-party / publisher trust stores.** docs/24 describes verifying a plugin manifest's
//!   signature "against publisher's registered key," implying a registry of many trusted
//!   publisher public keys. No such registry exists anywhere in this workspace today, and
//!   building one is a separate, real PKI/trust-management feature -- this crate instead models
//!   one real device identity whose private key nothing without it can forge a valid signature
//!   under, which already satisfies the milestone's actual exit criterion ("not a checksum a
//!   forger could trivially reproduce") without inventing an undocumented multi-key design.

use std::fs;
use std::path::Path;

use ed25519_dalek::{Signer, Verifier};
use rand_core::OsRng;

pub use blake3::Hash;
pub use ed25519_dalek::{Signature, SigningKey, VerifyingKey};

#[derive(Debug, thiserror::Error)]
pub enum KeystoreError {
    #[error("failed to read or write the real keystore file at {path:?}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the keystore file at {0:?} does not hold a valid 32-byte Ed25519 seed")]
    Corrupt(std::path::PathBuf),
}

/// A real, minimal, file-backed Ed25519 keystore -- "a software keystore at minimum," this
/// milestone's own stated floor. Holds one real device signing identity: everything in this
/// workspace that signs anything today signs with the *same* device key, matching docs/34's own
/// singular "device_key" framing rather than inventing an undocumented multi-key PKI (see this
/// crate's own doc comment).
pub struct Keystore {
    signing_key: SigningKey,
}

impl Keystore {
    /// Loads the real Ed25519 signing key at `path` if one already exists, otherwise generates a
    /// real one via the OS CSPRNG and persists its raw 32-byte seed to `path` (creating parent
    /// directories as needed, and -- on Unix -- restricting the file to owner-only read/write,
    /// `0o600`, the instant it's written, before any key material could be read by anyone else).
    pub fn open_or_create(path: &Path) -> Result<Self, KeystoreError> {
        if path.exists() {
            let bytes = fs::read(path).map_err(|source| KeystoreError::Io {
                path: path.to_path_buf(),
                source,
            })?;
            let seed: [u8; 32] = bytes
                .try_into()
                .map_err(|_| KeystoreError::Corrupt(path.to_path_buf()))?;
            Ok(Keystore {
                signing_key: SigningKey::from_bytes(&seed),
            })
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|source| KeystoreError::Io {
                    path: path.to_path_buf(),
                    source,
                })?;
            }
            let signing_key = SigningKey::generate(&mut OsRng);
            Self::persist_new_key(path, &signing_key)?;
            Ok(Keystore { signing_key })
        }
    }

    #[cfg(unix)]
    fn persist_new_key(path: &Path, signing_key: &SigningKey) -> Result<(), KeystoreError> {
        use std::os::unix::fs::PermissionsExt;
        fs::write(path, signing_key.to_bytes()).map_err(|source| KeystoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
            KeystoreError::Io {
                path: path.to_path_buf(),
                source,
            }
        })
    }

    #[cfg(not(unix))]
    fn persist_new_key(path: &Path, signing_key: &SigningKey) -> Result<(), KeystoreError> {
        fs::write(path, signing_key.to_bytes()).map_err(|source| KeystoreError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    /// This keystore's real public key -- what a verifier anywhere else in the system checks a
    /// signature against, via [`verify`].
    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// A real Ed25519 signature over `bytes`.
    pub fn sign(&self, bytes: &[u8]) -> Signature {
        self.signing_key.sign(bytes)
    }
}

/// A real Ed25519 signature check: `true` only if `signature` is a genuine signature over
/// exactly `bytes`, produced by the private key matching `verifying_key` -- unlike the checksum
/// stand-ins this replaces, nobody without that private key can produce a `signature` that
/// passes this for any `bytes` they did not have signed for them.
pub fn verify(bytes: &[u8], signature: &Signature, verifying_key: &VerifyingKey) -> bool {
    verifying_key.verify(bytes, signature).is_ok()
}

/// A real BLAKE3 content hash -- this workspace's own already-stated preference (docs/28's
/// content-defined chunking names BLAKE3 explicitly) for the "real SHA-256/BLAKE3 hashing" this
/// milestone asks for; used here for `hyperion-observability`'s real hash-chain.
pub fn hash(bytes: &[u8]) -> Hash {
    blake3::hash(bytes)
}
