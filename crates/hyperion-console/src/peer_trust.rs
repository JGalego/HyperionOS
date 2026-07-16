//! Real, persisted trust-on-first-use (TOFU) peer identity -- docs/998-roadmap.md's Social
//! pillar's own "real cross-instance discovery, identity, and trust" gap, closed for the
//! *identity* half the same well-established way SSH's own `known_hosts` does: the first time a
//! peer (identified by a caller-chosen id, e.g. `"host:port"`) is ever contacted, its real
//! Ed25519 public key is recorded; every later contact compares the newly presented key against
//! that record instead of trusting it blindly again. A key that suddenly changes is a real,
//! surfaced [`TrustOutcome::KeyMismatch`] — exactly the signal SSH's own "REMOTE HOST
//! IDENTIFICATION HAS CHANGED" warning gives, not silently overwritten or silently ignored.
//!
//! **This is identity continuity, not authorization.** Nothing here decides whether a peer
//! *should* be talked to — that's still the caller's own explicit `/a2a-call`/`/mcp-call`
//! decision (naming an exact host/port). This only answers "is this really the same peer I
//! talked to last time," and only once the caller has independently verified (via a real
//! signature) that the presented key is genuinely held by whoever replied — see this crate's own
//! `a2a` module for that half.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum PeerTrustError {
    #[error("failed to read or write the real peer trust store at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the peer trust store at {0:?} isn't valid JSON")]
    Corrupt(PathBuf),
}

/// What comparing a freshly presented public key against this store's own record produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustOutcome {
    /// No key was ever recorded for this peer id before -- it's been recorded now.
    FirstTrust,
    /// The presented key matches exactly what was recorded on a previous contact.
    Trusted,
    /// The presented key does *not* match what was recorded before -- a real, surfaced warning,
    /// never silently overwritten.
    KeyMismatch { previously_trusted_key_hex: String },
}

/// A real, file-backed (plain JSON -- these are public keys, not secrets, so no encryption-at-
/// rest is needed the way [`hyperion_crypto::SecretStore`] needs it) map of peer id to the real
/// hex-encoded Ed25519 public key first seen for it.
pub struct PeerTrustStore {
    path: PathBuf,
    trusted: HashMap<String, String>,
}

impl PeerTrustStore {
    /// Loads the real store at `path` if one already exists, otherwise starts a real, empty one
    /// (persisted only once a peer is actually trusted -- see [`Self::verify_or_trust`]).
    pub fn open_or_create(path: impl AsRef<Path>) -> Result<Self, PeerTrustError> {
        let path = path.as_ref().to_path_buf();
        let trusted = if path.exists() {
            let raw = std::fs::read_to_string(&path).map_err(|source| PeerTrustError::Io {
                path: path.clone(),
                source,
            })?;
            serde_json::from_str(&raw).map_err(|_| PeerTrustError::Corrupt(path.clone()))?
        } else {
            HashMap::new()
        };
        Ok(PeerTrustStore { path, trusted })
    }

    fn save(&self) -> Result<(), PeerTrustError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| PeerTrustError::Io {
                path: self.path.clone(),
                source,
            })?;
        }
        let raw = serde_json::to_string_pretty(&self.trusted)
            .expect("a HashMap<String, String> always serializes");
        std::fs::write(&self.path, raw).map_err(|source| PeerTrustError::Io {
            path: self.path.clone(),
            source,
        })
    }

    /// The real TOFU comparison: records `observed_key_hex` the first time `peer_id` is seen,
    /// confirms a match on every later call, and surfaces (never silently resolves) a mismatch.
    pub fn verify_or_trust(
        &mut self,
        peer_id: &str,
        observed_key_hex: &str,
    ) -> Result<TrustOutcome, PeerTrustError> {
        match self.trusted.get(peer_id) {
            None => {
                self.trusted
                    .insert(peer_id.to_string(), observed_key_hex.to_string());
                self.save()?;
                Ok(TrustOutcome::FirstTrust)
            }
            Some(known) if known == observed_key_hex => Ok(TrustOutcome::Trusted),
            Some(known) => Ok(TrustOutcome::KeyMismatch {
                previously_trusted_key_hex: known.clone(),
            }),
        }
    }

    /// Real, explicit revocation -- the user's own override once a [`TrustOutcome::KeyMismatch`]
    /// warning has been investigated and the new key is (or isn't) actually trusted; matches
    /// CLAUDE.md's own "every action must be inspectable/configurable" principle applied to a
    /// trust decision, not just an autonomous one. `true` if `peer_id` was really recorded.
    pub fn forget(&mut self, peer_id: &str) -> Result<bool, PeerTrustError> {
        let existed = self.trusted.remove(peer_id).is_some();
        if existed {
            self.save()?;
        }
        Ok(existed)
    }

    /// Every currently-trusted `(peer_id, key_hex)` pair, sorted by peer id -- the real
    /// inspectability half of the same principle [`Self::forget`]'s own doc comment names.
    pub fn trusted_peers(&self) -> Vec<(String, String)> {
        let mut peers: Vec<_> = self
            .trusted
            .iter()
            .map(|(id, key)| (id.clone(), key.clone()))
            .collect();
        peers.sort();
        peers
    }
}

/// Lowercase hex, no dependency needed for this one fixed-size-byte-array use case.
pub fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The inverse of [`encode_hex`] -- `None` for anything that isn't a real, even-length hex
/// string (an odd length or a non-hex character means a corrupt or tampered value, never
/// silently truncated or partially decoded).
pub fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}
