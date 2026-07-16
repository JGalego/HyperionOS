//! Hyperion L1-cross-cutting Privacy Architecture — Phase 8, third slice.
//!
//! Implements docs/16-privacy-architecture.md's canonical, three-tier
//! `PrivacyTier` gate and consent ledger — the "one real, general
//! privacy-tier enforcement mechanism" Phase 8 hardens, per docs/16's own
//! framing that Privacy Architecture sits "logically upstream of the
//! Model Router... none of them may bypass it, because none of them hold
//! key material or consent state directly."
//!
//! Real: [`routing::route_capability_call`] is docs/16 §5/§7's exact
//! algorithm — deny-by-default, "never assume consent" (an absent grant,
//! whether never-issued or revoked, always degrades, never defaults to
//! allow); [`consent::ConsentLedger`] backs it with a real, revocable
//! grant store; [`types::ResidencyTag`] structurally forbids
//! `Restricted`-classified objects from ever carrying `CloudAssisted` in
//! their allowed tiers, enforced at construction rather than trusted to
//! every call site; [`routing::filter_for_recipient`] implements docs/16
//! §5's least-privilege context assembly ("build for recipient contract,"
//! not filter-after), reporting every exclusion with a reason rather than
//! silently dropping it; [`erasure::erase`] implements docs/16 §5's
//! erasure request against the real `hyperion-knowledge-graph`, and for
//! `ErasureMode::SoftDelete` registers a real [33 — Rollback &
//! Recovery](../33-rollback-recovery.md) grace period via
//! `hyperion-recovery::RecoveryService` — the erasure is journaled as a
//! committed, undoable `ActionRecord`, not merely tombstoned with no way
//! back.
//!
//! **On `hyperion-model-router`/`hyperion-netstack`/`hyperion-federation`'s
//! own narrow, two-value `PrivacyTier{Local, ConsentedCloud}` gates**:
//! docs/16 is explicit that there should be "one routing decision," not
//! competing enforcement points. This crate is the canonical type and
//! algorithm going forward; those three crates' own gates predate this
//! one, are load-bearing in their own already-green test suites, and are
//! narrow simplifications each already documents as such in its own
//! crate doc — rewiring three already-shipped, CI-passing crates to
//! depend on a brand-new fourth crate mid-hardening-pass is a real,
//! separate migration this slice does not attempt. New integration work
//! (this phase's threat-model regression suite, and any future crate)
//! should depend on `hyperion-privacy`'s types, not invent a fifth
//! simplification.
//!
//! Deliberately deferred, and why:
//!
//! - **Real encryption at rest, key wrapping, and Shamir secret-sharing
//!   recovery.** `hyperion-crypto` (Phase 8/M9) means "no crate in this
//!   workspace performs real cryptography yet" is no longer why this is
//!   deferred, but adding [`types::ConsentGrant`]'s `proof: Signature`
//!   (docs/16 §4) today would be signing with no real verifier: this
//!   ledger is purely local and capability-gated, with no import/sync
//!   path anywhere that would ever check such a signature — the same
//!   gap as the bullet below (multi-device sync doesn't exist here yet).
//!   A real signature with nothing to verify it against is exactly the
//!   kind of mechanism-with-no-consumer this workspace's own convention
//!   avoids; it belongs with that sync work, not bolted on alone.
//!   [`types::ResidencyTag`]'s `encryption_key_ref` is a further, larger
//!   gap: real encryption of arbitrary Knowledge Graph node metadata
//!   would need to intercept `hyperion-storage`'s own WAL write/read
//!   path with per-tag key material — a real, separate feature, not a
//!   field addition. Key wrapping and Shamir secret-sharing recovery
//!   remain out of scope the same way M9's own completion note already
//!   scoped them: docs/28's fuller DEK/KEK/master-key hierarchy, deferred
//!   there as "a real, separate, larger feature," not required by any
//!   milestone's own exit criteria.
//! - **`SyncEnvelope`/multi-device CRDT gossip and erasure propagation
//!   across devices.** `ErasureReceipt` here has no
//!   `propagated_to_devices` field — this crate has no multi-device sync
//!   model to propagate across; `hyperion-federation` is where multiple
//!   devices exist in this workspace, and it isn't wired to this crate.
//! - **Physical deletion / `CryptoShred`'s wire-indistinguishability
//!   guarantee.** `hyperion-knowledge-graph` has no node-delete operation
//!   (only edges tombstone); [`erasure::erase`] overwrites a node's
//!   current metadata with a tombstone-shaped placeholder — a real,
//!   observable state change, not a byte-level history deletion, and
//!   nothing here disguises an erasure's network/timing signature (moot
//!   without a real transport anyway).
//! - ~~**Real crash-recovery timers expiring the grace period.**~~ — now real:
//!   [`erasure::expire_lapsed_soft_deletes`] is the real, caller-driven clock (matching this
//!   workspace's hosted-simulator convention of a caller-supplied `now` rather than a real
//!   background thread): every soft-delete `ActionRecord` still `Committed` whose age has
//!   reached a caller-chosen `grace_period_secs` is sealed via
//!   `hyperion_recovery::RecoveryService::expire` (a new state transition that crate gained for
//!   exactly this), after which it can never be undone again — the same irreversibility
//!   `ErasureMode::CryptoShred` already had from the start. Named simplification: docs/16 §4's
//!   own `ErasureRequest.grace_period` is a per-request field; this sweep applies one
//!   caller-supplied duration uniformly to every pending soft-delete, since `ActionRecord` has no
//!   per-action grace-period field of its own to vary it by (deliberately — that type stays
//!   privacy-agnostic; many other crates journal through it too). `hyperion-recovery`'s own
//!   separate "retention classes, compaction, and pinning enforcement beyond a boolean flag"
//!   deferral (recovery points/the action journal simply accumulating for the process lifetime)
//!   is a different, still-open gap — closing this one doesn't imply that one is closed too.
//! - **`memory.*`/`knowledgeGraph.*` full Inspect/Edit/Export API
//!   surface** (docs/16 §6) — only `erase` is implemented; `inspect`/
//!   `edit`/`export` are direct callers of `hyperion-knowledge-graph`'s
//!   own `get`/`put_node`/`query` today, with no privacy-specific
//!   wrapper needed yet.

mod consent;
mod erasure;
mod routing;
mod types;

pub use consent::ConsentLedger;
pub use erasure::{erase, expire_lapsed_soft_deletes};
pub use routing::{filter_for_recipient, route_capability_call};
pub use types::{
    ConsentGrant, DataScope, DegradeReason, ErasureMode, ErasureReceipt, PrivacyError,
    PrivacyProfile, PrivacyTier, ResidencyTag, RoutingDecision, SensitivityClass,
};
