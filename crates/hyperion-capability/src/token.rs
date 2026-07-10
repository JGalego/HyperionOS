use std::time::Instant;

use crate::types::{ObjectId, RightsMask, TokenId, TrustBoundaryId};

/// An unforgeable reference to exactly one kernel object plus a rights mask,
/// per docs/03-kernel-architecture.md §Data Structures.
///
/// Every field is private and every constructor is `pub(crate)`: the only
/// place a `CapabilityToken` can come into existence is [`crate::CapabilityMonitor`].
/// User code can copy, pass, and inspect a token it already holds, but it can
/// never synthesize one — the same guarantee docs/03 describes for the real
/// kernel, given here by Rust's module privacy instead of hardware privilege.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityToken {
    pub(crate) token_id: TokenId,
    pub(crate) object_id: ObjectId,
    pub(crate) rights: RightsMask,
    pub(crate) generation: u64,
    pub(crate) origin: TrustBoundaryId,
    pub(crate) expiry: Option<Instant>,
}

impl CapabilityToken {
    /// Identity of this specific delegation-graph node. Not part of the
    /// upstream spec's struct; added so revocation can be scoped per-node
    /// rather than per-`ObjectId` (see [`crate::types::TokenId`]'s docs).
    pub fn token_id(&self) -> TokenId {
        self.token_id
    }

    pub fn object_id(&self) -> ObjectId {
        self.object_id
    }

    pub fn rights(&self) -> RightsMask {
        self.rights
    }

    /// Generation snapshot captured when this token was minted or derived.
    /// Compared against the monitor's live per-node generation on every
    /// invocation; a mismatch means an ancestor (or this token itself) was
    /// revoked since this snapshot was taken.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn origin(&self) -> TrustBoundaryId {
        self.origin
    }

    pub fn expiry(&self) -> Option<Instant> {
        self.expiry
    }

    pub fn is_expired(&self) -> bool {
        self.expiry.is_some_and(|t| Instant::now() > t)
    }
}
