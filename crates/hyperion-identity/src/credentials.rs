//! What each person has set as their passphrase, if anything.
//!
//! Kept beside [`crate::PrincipalRegistry`] rather than inside it, because the two answer different
//! questions and one of them is a secret. The register of who uses this device is ordinary data --
//! names and integers, safe to read. This is the file an attacker wants.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use hyperion_crypto::{CredentialError, Keystore, PassphraseVerifier};

use crate::types::{UserId, UserIdError};

#[derive(Debug, thiserror::Error)]
pub enum CredentialStoreError {
    #[error("{0}")]
    UserId(#[from] UserIdError),
    #[error("{0}")]
    Credential(#[from] CredentialError),
    #[error("I couldn't read this device's passphrases: {0}")]
    Read(String),
    #[error("I couldn't save that passphrase: {0}")]
    Write(String),
}

/// Whether a person may become who they say they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthOutcome {
    /// They proved it.
    Verified,
    /// They did not. Deliberately one outcome rather than "wrong passphrase" versus "no such
    /// person": telling those apart lets someone enumerate who exists on a device by trying names.
    Refused,
    /// Nobody has set a passphrase for this person, so there is nothing to prove.
    ///
    /// Not a failure, and not a silent success either — it is the honest state of a device that has
    /// principals but no credentials yet, and the caller decides what to do about it. What must
    /// *not* happen is for it to be quietly treated as [`Self::Verified`], which is how a system
    /// ends up authenticating nobody while appearing to authenticate everybody.
    NoCredential,
}

/// The passphrases this device knows, one per person at most.
pub struct CredentialStore {
    path: PathBuf,
    verifiers: BTreeMap<UserId, String>,
}

impl CredentialStore {
    pub fn open_or_create(path: impl AsRef<Path>) -> Result<Self, CredentialStoreError> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Ok(CredentialStore {
                path,
                verifiers: BTreeMap::new(),
            });
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| CredentialStoreError::Read(e.to_string()))?;
        let value: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| CredentialStoreError::Read(e.to_string()))?;
        let entries = value
            .get("passphrases")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                CredentialStoreError::Read("it isn't in the shape I wrote it".to_string())
            })?;

        let mut verifiers = BTreeMap::new();
        for (name, stored) in entries {
            let user = UserId::new(name)?;
            let stored = stored.as_str().ok_or_else(|| {
                CredentialStoreError::Read(format!("{name}'s entry isn't a passphrase record"))
            })?;
            // Checked when the file is read, not when somebody tries to log in: better to notice a
            // corrupt credentials file at startup than to have a correct passphrase mysteriously
            // stop working.
            PassphraseVerifier::parse(stored)?;
            verifiers.insert(user, stored.to_string());
        }
        Ok(CredentialStore { path, verifiers })
    }

    /// Sets (or replaces) one person's passphrase.
    pub fn set(
        &mut self,
        user: &UserId,
        passphrase: &str,
        device_key: &Keystore,
    ) -> Result<(), CredentialStoreError> {
        let verifier = hyperion_crypto::hash_passphrase(passphrase, device_key)?;
        self.verifiers
            .insert(user.clone(), verifier.as_str().to_string());
        self.persist()
    }

    /// Whether this person has set one at all.
    pub fn has_credential(&self, user: &UserId) -> bool {
        self.verifiers.contains_key(user)
    }

    /// Checks a passphrase against what is stored.
    pub fn authenticate(
        &self,
        user: &UserId,
        passphrase: &str,
        device_key: &Keystore,
    ) -> AuthOutcome {
        let Some(stored) = self.verifiers.get(user) else {
            return AuthOutcome::NoCredential;
        };
        let Ok(verifier) = PassphraseVerifier::parse(stored) else {
            // A stored verifier that no longer parses is a refusal, never an opening.
            return AuthOutcome::Refused;
        };
        if hyperion_crypto::verify_passphrase(passphrase, &verifier, device_key) {
            AuthOutcome::Verified
        } else {
            AuthOutcome::Refused
        }
    }

    /// Forgets one person's passphrase, so they are unprotected again rather than locked out.
    pub fn clear(&mut self, user: &UserId) -> Result<(), CredentialStoreError> {
        self.verifiers.remove(user);
        self.persist()
    }

    fn persist(&self) -> Result<(), CredentialStoreError> {
        let passphrases: serde_json::Map<String, serde_json::Value> = self
            .verifiers
            .iter()
            .map(|(user, stored)| (user.to_string(), serde_json::json!(stored)))
            .collect();
        let document = serde_json::json!({ "passphrases": passphrases });
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| CredentialStoreError::Write(e.to_string()))?;
        }
        std::fs::write(
            &self.path,
            serde_json::to_vec_pretty(&document)
                .map_err(|e| CredentialStoreError::Write(e.to_string()))?,
        )
        .map_err(|e| CredentialStoreError::Write(e.to_string()))
    }
}
