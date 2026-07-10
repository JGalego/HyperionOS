use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use bitflags::bitflags;

/// Opaque, monitor-assigned identifier for a kernel object (a page, a thread,
/// an endpoint, a device register range, ...). Never user-synthesizable:
/// the only way to obtain one is to receive a [`CapabilityToken`] naming it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectId(pub u64);

/// Which Trust Boundary (process, container, VM, or remote host) a token
/// was minted for, per docs/02-core-architecture.md's Trust Boundary vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TrustBoundaryId(pub u64);

/// Unique identity of one specific capability *instance* (a node in the
/// revocation graph), distinct from `ObjectId`.
///
/// docs/03-kernel-architecture.md's pseudocode checks a token's staleness via
/// `registry_generation(object_id)`, i.e. one generation counter per object.
/// That is a design bug, not just a translation gap: many independent
/// delegation chains can name the same `ObjectId` (a root capability and
/// several attenuated children handed to unrelated holders), and a single
/// per-object counter would mean revoking any one of them invalidates every
/// other holder's token for that object too — including siblings and the
/// very parent that did the revoking. Generation must be tracked per
/// revocation-graph *node* so that revoking a token invalidates exactly its
/// own descendants, per the same paragraph's "cascading... whole delegated
/// subtree" claim. See the accompanying fix in docs/03-kernel-architecture.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TokenId(pub u64);

impl TokenId {
    pub(crate) fn next() -> TokenId {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        TokenId(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

bitflags! {
    /// Rights mask carried by a [`CapabilityToken`]. Attenuation (`cap_derive`)
    /// may only narrow this set, never widen it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub struct RightsMask: u32 {
        const READ   = 0b0000_0001;
        const WRITE  = 0b0000_0010;
        const MAP    = 0b0000_0100;
        const EXEC   = 0b0000_1000;
        const GRANT  = 0b0001_0000;
        const REVOKE = 0b0010_0000;
    }
}

/// An operation requested against the object a capability names. Kept opaque
/// and small at this layer: it is dispatched by whichever subsystem owns the
/// object (IPC endpoint, scheduler resource, ...), not by the capability core
/// itself — see the crate-level docs for why `cap_invoke` is a check, not a
/// dispatcher, in this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Operation(pub u32);

/// Everything that can go wrong presenting a capability, per
/// docs/03-kernel-architecture.md's Interfaces/APIs section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Fault {
    #[error("no such capability")]
    NoSuchCapability,
    #[error("capability has been revoked")]
    Revoked,
    #[error("capability has expired")]
    Expired,
    #[error("insufficient rights for this operation")]
    InsufficientRights,
    #[error("cannot escalate rights beyond parent capability")]
    CannotEscalate,
}

pub(crate) fn min_expiry(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}
