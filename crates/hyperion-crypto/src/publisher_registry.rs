//! A real registry of trusted publisher public keys -- this crate's own previously-named
//! "multi-party / publisher trust stores" gap, closed for a caller that wants real per-publisher
//! trust instead of this workspace's single-device-identity default. docs/24's own "verify
//! against publisher's registered key" framing is exactly what [`PublisherRegistry`] is: register
//! a publisher's real, trusted [`crate::VerifyingKey`] once, look it up by the same id a manifest
//! declares. Real publisher onboarding ceremony/rotation policy/revocation are all out of scope --
//! this is the trust store itself, not the process that populates it.

use std::collections::HashMap;

use crate::VerifyingKey;

/// A real, in-memory map from publisher id to trusted [`VerifyingKey`]. `Default`/[`Self::new`]
/// start empty -- an unregistered publisher id is a real, honest "not trusted," never a silent
/// fallback to some other key.
#[derive(Debug, Default)]
pub struct PublisherRegistry {
    keys: HashMap<String, VerifyingKey>,
}

impl PublisherRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `publisher_id`'s real, trusted public key -- overwrites any previous key
    /// registered under the same id. That's a real, deliberate key-rotation path, not an
    /// accidental one: a caller re-registering the same id is assumed to mean it, the same
    /// "last write wins" convention [`crate::sync_envelope`]'s own key derivation context
    /// switching relies on implicitly.
    pub fn register(&mut self, publisher_id: impl Into<String>, key: VerifyingKey) {
        self.keys.insert(publisher_id.into(), key);
    }

    /// The real, trusted public key for `publisher_id`, if this registry has ever registered
    /// one -- `None` for an unrecognized publisher.
    pub fn verifying_key_for(&self, publisher_id: &str) -> Option<VerifyingKey> {
        self.keys.get(publisher_id).cloned()
    }
}
