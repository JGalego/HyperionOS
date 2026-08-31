//! Proving who somebody is — docs/998-roadmap.md's App Builder T4.
//!
//! `hyperion-identity` establishes *that* there are principals and keeps their things apart. It
//! says so plainly in its own doc comment: it separates people, it does not protect them from each
//! other, because anyone at the console can claim to be anyone. This is the other half.
//!
//! ## Why Argon2id and not the primitives already here
//!
//! Every other primitive in this crate is deliberately fast. BLAKE3 hashes gigabytes a second, and
//! `Keystore::derive_key` is one BLAKE3 call. That is exactly wrong for a password: an attacker who
//! obtains the stored verifiers gets to guess offline, at whatever rate the hash allows, and a fast
//! hash means billions of guesses. A password verifier's entire job is to be slow and
//! memory-hard. Reaching for a fast hash "used carefully" is the classic way this gets built wrong.
//!
//! ## What the device key still adds
//!
//! The Argon2 salt is per-credential and random, as it must be. On top of that, the passphrase is
//! *peppered* with a device-bound secret from [`crate::Keystore::derive_key`] before hashing. That
//! is not a substitute for the KDF; it means a stolen credentials file alone is not enough to
//! mount an offline attack at all — an attacker needs the device's signing key as well, which is
//! the one secret this workspace already treats as the root of everything.

use argon2::password_hash::{rand_core::OsRng, SaltString};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};

use crate::Keystore;

/// Domain-separates the pepper from every other caller of `Keystore::derive_key`.
const PEPPER_CONTEXT: &str = "hyperion.credentials.pepper.v1";

/// The shortest passphrase this will accept.
///
/// A floor, not advice. Argon2id makes guessing expensive per attempt, which does nothing for a
/// passphrase short enough to be in the first thousand guesses. Deliberately stated as a real
/// refusal rather than a warning nobody reads.
pub const MIN_PASSPHRASE_LEN: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CredentialError {
    #[error("a passphrase needs to be at least {MIN_PASSPHRASE_LEN} characters")]
    TooShort,
    #[error("that isn't a passphrase I stored")]
    Malformed,
    #[error("I couldn't secure that passphrase: {0}")]
    Hashing(String),
}

/// A stored proof that somebody knows a passphrase, which is never the passphrase itself.
///
/// The string inside is the PHC-format Argon2id output: algorithm, parameters, salt and hash. It is
/// safe to persist, and deliberately carries its own parameters so credentials stored under older
/// settings keep verifying after those settings change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassphraseVerifier(String);

impl PassphraseVerifier {
    /// The stored form, for writing to disk.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Reads back what [`Self::as_str`] wrote, rejecting anything that is not a real PHC string
    /// rather than storing it and failing at the moment somebody tries to log in.
    pub fn parse(stored: &str) -> Result<Self, CredentialError> {
        PasswordHash::new(stored).map_err(|_| CredentialError::Malformed)?;
        Ok(PassphraseVerifier(stored.to_string()))
    }
}

/// Peppers a passphrase with this device's own identity before it is hashed.
fn peppered(passphrase: &str, device_key: &Keystore) -> Vec<u8> {
    let pepper = device_key.derive_key(PEPPER_CONTEXT);
    let mut input = Vec::with_capacity(passphrase.len() + pepper.len());
    input.extend_from_slice(passphrase.as_bytes());
    input.extend_from_slice(&pepper);
    input
}

/// Turns a passphrase into something safe to store.
///
/// Each call generates a fresh random salt, so the same passphrase chosen by two people (or the
/// same person twice) produces different stored verifiers and neither reveals that the other
/// matches.
pub fn hash_passphrase(
    passphrase: &str,
    device_key: &Keystore,
) -> Result<PassphraseVerifier, CredentialError> {
    // Counted in characters, not bytes: a short passphrase of multi-byte characters is still short,
    // and a byte-length check would quietly accept it.
    if passphrase.chars().count() < MIN_PASSPHRASE_LEN {
        return Err(CredentialError::TooShort);
    }
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(&peppered(passphrase, device_key), &salt)
        .map_err(|e| CredentialError::Hashing(e.to_string()))?;
    Ok(PassphraseVerifier(hash.to_string()))
}

/// Whether `passphrase` is the one `verifier` was made from.
///
/// Returns a plain `bool` rather than a `Result` distinguishing "wrong passphrase" from "malformed
/// stored verifier": a caller must not be able to tell those apart by accident, and every reason to
/// refuse should look identical from outside. `Argon2::verify_password` compares in constant time.
pub fn verify_passphrase(
    passphrase: &str,
    verifier: &PassphraseVerifier,
    device_key: &Keystore,
) -> bool {
    let Ok(parsed) = PasswordHash::new(&verifier.0) else {
        return false;
    };
    Argon2::default()
        .verify_password(&peppered(passphrase, device_key), &parsed)
        .is_ok()
}
