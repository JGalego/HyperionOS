//! A real, Ed25519-signed catalog of known-good model sources -- this crate's own previously-
//! unnamed gap: [`crate::candle_backend`]'s own real Hugging Face Hub repo/revision/filename
//! constants (`stories15M.bin`, the real quantized GGUF checkpoint, the real safetensors export)
//! were hardcoded directly in that module with no real integrity check on what actually gets
//! downloaded and loaded, and no single place a caller could inspect "what model sources does
//! this build actually trust." [`ModelCatalog`] is that place: a real list of named sources, each
//! naming exactly where its real weights come from (`repo`/`revision`/`filename`, `hf-hub`'s own
//! addressing scheme) and a real BLAKE3 content hash the downloaded bytes must match before
//! [`crate::candle_backend::CandleBackend`] ever loads them -- the same "capability-based,
//! auditable, reversible" bar CLAUDE.md sets for every other action in this workspace, applied to
//! "which model weights get to run."
//!
//! [`sign_catalog`]/[`verify_catalog`] mirror [`crate::registry::sign`]/[`crate::registry::verify`]'s
//! own real Ed25519-over-canonical-bytes shape exactly -- a catalog is only trustworthy if it was
//! really signed by a real device/publisher key, not merely well-formed.
//!
//! Deliberately not wired into a persisted, user-editable file yet -- see backlog item 28's own
//! `model_selection.json` follow-up; this module is the real, standalone primitive that follow-up
//! builds on, proven here against [`crate::candle_backend`]'s own three already-real, already-
//! verified download constants.

use std::path::Path;

use hyperion_crypto::{Keystore, Signature, VerifyingKey};

/// The real file format a [`ModelCatalogEntry`] names -- matches [`crate::candle_backend`]'s own
/// three real loading paths exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFormat {
    /// Karpathy's own bespoke `llama2.c` binary layout ([`crate::candle_backend::CandleBackend::load`]).
    Llama2CBinary,
    /// A real quantized GGUF file in llama.cpp's own standard format
    /// ([`crate::candle_backend::CandleBackend::load_gguf`]).
    Gguf,
    /// A real Hugging Face `transformers`-format safetensors export
    /// ([`crate::candle_backend::CandleBackend::load_safetensors`]).
    Safetensors,
}

/// One real, verifiable model source -- exactly enough to name where a real checkpoint comes
/// from and what its real bytes must hash to once downloaded, never anything about the weights'
/// own internal architecture (that's [`crate::candle_backend`]'s concern, derived from the real
/// file itself at load time).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCatalogEntry {
    /// A short, stable name a caller selects this entry by (docs/998-roadmap.md backlog item
    /// 28's own `model_selection.json` follow-up references entries by this, not by
    /// repo/filename directly).
    pub name: String,
    /// A real `"owner/name"` Hugging Face Hub repo id.
    pub repo: String,
    /// A real, pinned commit hash -- never a mutable ref like `"main"`, the same
    /// zero-network-after-first-fetch property [`crate::candle_backend`]'s own revision
    /// constants already established.
    pub revision: String,
    pub filename: String,
    pub format: ModelFormat,
    /// A real BLAKE3 content hash (lowercase hex, [`hyperion_crypto::hash`]'s own `to_hex()`
    /// output) of the exact real bytes `repo`/`revision`/`filename` names -- computed once,
    /// against the real downloaded file, before ever being written here.
    pub blake3_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCatalog {
    pub entries: Vec<ModelCatalogEntry>,
}

impl ModelCatalog {
    /// The one real, verified catalog this crate ships -- exactly
    /// [`crate::candle_backend`]'s own three already-real, already-boot-tested download
    /// constants, now named and hash-pinned in one inspectable place instead of scattered
    /// `const` declarations with no cross-check.
    pub fn built_in() -> Self {
        ModelCatalog {
            entries: vec![
                ModelCatalogEntry {
                    name: "stories15m-bin".to_string(),
                    repo: "karpathy/tinyllamas".to_string(),
                    revision: "0bd21da7698eaf29a0d7de3992de8a46ef624add".to_string(),
                    filename: "stories15M.bin".to_string(),
                    format: ModelFormat::Llama2CBinary,
                    blake3_hex: "0ae7339518cb124bb9a8fcef88e5dfb615d9e56c55a118e55817dd077c77455d"
                        .to_string(),
                },
                ModelCatalogEntry {
                    name: "stories15m-gguf".to_string(),
                    repo: "klosax/tinyllamas-stories-gguf".to_string(),
                    revision: "0d3726e5a1402ea8d8663acaef0878106d716d5e".to_string(),
                    filename: "tinyllamas-stories-15m-f32.gguf".to_string(),
                    format: ModelFormat::Gguf,
                    blake3_hex: "2a9a24c4d2540c59e4aba246a58e22f18bd492cc83393ddaa726e54aa3effaed"
                        .to_string(),
                },
                ModelCatalogEntry {
                    name: "stories15m-safetensors".to_string(),
                    repo: "Xenova/llama2.c-stories15M".to_string(),
                    revision: "17c2f1eabe1e163acc15ad35e225794e7b907682".to_string(),
                    filename: "model.safetensors".to_string(),
                    format: ModelFormat::Safetensors,
                    blake3_hex: "f4649731ab84098843b28861da9caa93fcaa5e0de2d4db587f097f7d35b1ca35"
                        .to_string(),
                },
            ],
        }
    }

    /// The one real entry named `name`, if this catalog has one.
    pub fn find(&self, name: &str) -> Option<&ModelCatalogEntry> {
        self.entries.iter().find(|e| e.name == name)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ModelCatalogError {
    #[error(
        "the real downloaded file for {name:?} does not match the catalog's own pinned hash -- \
         expected {expected}, got {actual}"
    )]
    HashMismatch {
        name: String,
        expected: String,
        actual: String,
    },
    #[error("reading the downloaded file to verify its hash: {0}")]
    Io(#[from] std::io::Error),
}

/// The exact fields a real signature is produced/verified over -- every real, trust-relevant
/// field of every entry, in a fixed, deterministic order (registration order, not sorted: two
/// catalogs with the same entries in a different order are different signed artifacts, matching
/// [`crate::registry::sign`]'s own "exactly these bytes" precedent).
fn canonical_bytes(catalog: &ModelCatalog) -> Vec<u8> {
    let mut bytes = Vec::new();
    for entry in &catalog.entries {
        for field in [
            entry.name.as_str(),
            entry.repo.as_str(),
            entry.revision.as_str(),
            entry.filename.as_str(),
            entry.blake3_hex.as_str(),
        ] {
            bytes.extend_from_slice(field.as_bytes());
            bytes.push(0);
        }
        bytes.extend_from_slice(&(entry.format as u64).to_le_bytes());
    }
    bytes
}

/// A real Ed25519 signature over `catalog`'s own canonical bytes -- the same real signing
/// primitive [`crate::registry::sign`] already established for a [`crate::types::ModelDescriptor`],
/// applied here to a whole catalog of model sources instead of one descriptor.
pub fn sign_catalog(catalog: &ModelCatalog, keystore: &Keystore) -> Signature {
    keystore.sign(&canonical_bytes(catalog))
}

/// `true` only if `signature` is a genuine signature over exactly `catalog`'s own canonical
/// bytes, produced by the private key matching `verifying_key` -- a tampered entry (a swapped
/// hash, a different repo) is rejected, not silently accepted.
pub fn verify_catalog(
    catalog: &ModelCatalog,
    signature: &Signature,
    verifying_key: &VerifyingKey,
) -> bool {
    hyperion_crypto::verify(&canonical_bytes(catalog), signature, verifying_key)
}

/// Verifies `path`'s real content hash against `entry`'s own pinned [`ModelCatalogEntry::blake3_hex`]
/// -- the real supply-chain integrity check a bare, unverified `hf-hub` download never had: a
/// corrupted or substituted download is caught before [`crate::candle_backend::CandleBackend`]
/// ever loads it, not silently trusted just because the filename matched.
pub fn verify_file_hash(path: &Path, entry: &ModelCatalogEntry) -> Result<(), ModelCatalogError> {
    let bytes = std::fs::read(path)?;
    let actual = hyperion_crypto::hash(&bytes).to_hex().to_string();
    if actual != entry.blake3_hex {
        return Err(ModelCatalogError::HashMismatch {
            name: entry.name.clone(),
            expected: entry.blake3_hex.clone(),
            actual,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_crypto::Keystore;

    fn catalog() -> ModelCatalog {
        ModelCatalog {
            entries: vec![ModelCatalogEntry {
                name: "test-entry".to_string(),
                repo: "someone/somewhere".to_string(),
                revision: "abc123".to_string(),
                filename: "weights.gguf".to_string(),
                format: ModelFormat::Gguf,
                blake3_hex: "deadbeef".to_string(),
            }],
        }
    }

    #[test]
    fn a_real_signature_verifies_against_the_real_signing_keys_own_verifying_key() {
        let dir = tempfile::tempdir().unwrap();
        let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
        let catalog = catalog();

        let signature = sign_catalog(&catalog, &keystore);
        assert!(verify_catalog(
            &catalog,
            &signature,
            &keystore.verifying_key()
        ));
    }

    #[test]
    fn a_tampered_entry_fails_verification() {
        let dir = tempfile::tempdir().unwrap();
        let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
        let catalog = catalog();
        let signature = sign_catalog(&catalog, &keystore);

        let mut tampered = catalog.clone();
        tampered.entries[0].blake3_hex = "0000000000".to_string();
        assert!(!verify_catalog(
            &tampered,
            &signature,
            &keystore.verifying_key()
        ));
    }

    #[test]
    fn a_signature_from_a_different_key_fails_verification() {
        let dir = tempfile::tempdir().unwrap();
        let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
        let other_dir = tempfile::tempdir().unwrap();
        let other_keystore =
            Keystore::open_or_create(&other_dir.path().join("device.key")).unwrap();
        let catalog = catalog();

        let signature = sign_catalog(&catalog, &other_keystore);
        assert!(!verify_catalog(
            &catalog,
            &signature,
            &keystore.verifying_key()
        ));
    }

    #[test]
    fn built_in_names_every_real_candle_backend_download_constant() {
        let catalog = ModelCatalog::built_in();
        assert!(catalog.find("stories15m-bin").is_some());
        assert!(catalog.find("stories15m-gguf").is_some());
        assert!(catalog.find("stories15m-safetensors").is_some());
        assert!(catalog.find("no-such-entry").is_none());
    }

    #[test]
    fn a_file_matching_its_pinned_hash_verifies() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("weights.bin");
        std::fs::write(&path, b"real content").unwrap();
        let expected = hyperion_crypto::hash(b"real content").to_hex().to_string();

        let entry = ModelCatalogEntry {
            name: "test".to_string(),
            repo: "r".to_string(),
            revision: "v".to_string(),
            filename: "weights.bin".to_string(),
            format: ModelFormat::Gguf,
            blake3_hex: expected,
        };
        assert!(verify_file_hash(&path, &entry).is_ok());
    }

    #[test]
    fn a_file_not_matching_its_pinned_hash_is_a_real_hash_mismatch_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("weights.bin");
        std::fs::write(&path, b"tampered content").unwrap();

        let entry = ModelCatalogEntry {
            name: "test".to_string(),
            repo: "r".to_string(),
            revision: "v".to_string(),
            filename: "weights.bin".to_string(),
            format: ModelFormat::Gguf,
            blake3_hex: "0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
        };
        assert!(matches!(
            verify_file_hash(&path, &entry),
            Err(ModelCatalogError::HashMismatch { .. })
        ));
    }
}
