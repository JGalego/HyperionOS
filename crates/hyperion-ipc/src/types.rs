use bitflags::bitflags;

/// Which wire schema a [`crate::Channel`] speaks — a compiled contract id in
/// the real system (26-apis.md), an opaque tag here since Phase 1 has no
/// Capability contract compiler yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SchemaId(pub u32);

/// What a [`crate::Channel`] is for, per docs/30-ipc-framework.md §Data
/// Structures. `channel_open` checks the caller's rights the same way for
/// both today (both need at least `WRITE` to send into the endpoint), but
/// the class still gates which of `ipc_call` / `ipc_notify` a channel may be
/// used for, and future rights refinement (e.g. requiring `GRANT` for
/// `Call` channels) has somewhere to attach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelClass {
    Call,
    Notify,
}

/// docs/30-ipc-framework.md's Trust Boundary cases: this document adds the
/// remote-host case on top of 03-kernel-architecture.md's process/container/VM
/// spectrum. Phase 1 is single-host only — `Remote` is deferred to
/// [21-distributed-execution.md]'s federation work — so only `Local` exists
/// for now; the enum is kept (rather than omitted) so `Channel`'s shape
/// doesn't change when federation lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Local,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FrameFlags: u8 {
        const CALL       = 0b0000_0001;
        const REPLY      = 0b0000_0010;
        const NOTIFY     = 0b0000_0100;
        const ERROR      = 0b0000_1000;
        const HAS_REGION = 0b0001_0000;
    }
}

/// Everything that can go wrong at the IPC layer, per
/// docs/30-ipc-framework.md §Interfaces / APIs and §Failure Modes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IpcFault {
    /// `channel_open` collapses "wrong token," "revoked token," and "no such
    /// endpoint" into this single variant, deliberately — discovery must be
    /// capability-gated, not just access, per docs/30 §Architecture's
    /// "Capability-scoped discovery." A caller must not be able to
    /// distinguish "you got the token wrong" from "nothing is there."
    #[error("no such capability")]
    NoSuchCapability,
    #[error("peer is unreachable")]
    PeerUnreachable,
    #[error("call timed out waiting for a reply")]
    Timeout,
    #[error("schema mismatch")]
    SchemaMismatch,
    /// A capability-layer rejection surfaced *after* a channel was already
    /// open — e.g. the caller's token was revoked between `channel_open` and
    /// this specific call, caught by the receiving server re-validating the
    /// live capability on every invocation (docs/03 §Security
    /// Considerations' "revocation races... closed by checking the live
    /// revocation graph generation on every invocation, not a cached copy").
    #[error("capability check failed: {0}")]
    Kernel(#[from] hyperion_capability::Fault),
}
