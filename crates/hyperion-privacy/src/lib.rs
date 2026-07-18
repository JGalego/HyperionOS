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
//! back. `src/bin/hyperion-privacy-service.rs` (2026-07-16) is this crate's own real
//! `hyperion-supervisor` (M5) process entry point — a third, independent proof (after
//! `hyperion-observability`/`hyperion-explainability`) that a Phase 2-10 subsystem runs
//! unmodified as a real, capability-scoped supervised process; its own one real unit of work
//! requests a real `ConsentGrant` and shows `route_capability_call`'s real decision genuinely
//! reflecting it.
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
//! - ~~**`ConsentGrant.proof`.**~~ (2026-07-18) — now real: [`types::ConsentGrant`]'s
//!   `proof: Signature` (docs/16 §4) is minted by [`consent::ConsentLedger::request`] using the
//!   issuing device's own `hyperion_crypto::Keystore`, over the grant's own canonical bytes
//!   (mirroring `hyperion_plugin_framework`'s own signed-manifest convention). The real consumer
//!   this crate's own doc previously said didn't exist yet: [`consent::ConsentLedger::import`]
//!   verifies that signature against a caller-supplied `VerifyingKey` before ever trusting a
//!   grant this ledger didn't itself mint — the shape a grant relayed from another device over
//!   `hyperion-federation`'s own real, already-signed `SyncEnvelope` transport needs. Real ambient
//!   Knowledge Graph replication across devices now exists (`hyperion_federation::kg_sync`,
//!   2026-07-18), but this crate still has no *automatic* transport wiring into it (a
//!   `ConsentGrant` isn't a Knowledge Graph node/edge `kg_sync` replicates) — `import` is the
//!   real, callable, independently-tested building block a future consent-specific transport
//!   would use, not the transport-level automation itself.
//! - **Real encryption at rest, key wrapping, and Shamir secret-sharing
//!   recovery.** [`types::ResidencyTag`]'s `encryption_key_ref` remains a
//!   gap: real encryption of arbitrary Knowledge Graph node metadata
//!   would need to intercept `hyperion-storage`'s own WAL write/read
//!   path with per-tag key material (narrower than `hyperion-storage::StorageEngine::
//!   open_encrypted`'s own whole-graph key, landed 2026-07-17) — a real, separate feature, not a
//!   field addition. Key wrapping and Shamir secret-sharing recovery
//!   remain out of scope the same way M9's own completion note already
//!   scoped them: docs/28's fuller DEK/KEK/master-key hierarchy, deferred
//!   there as "a real, separate, larger feature," not required by any
//!   milestone's own exit criteria.
//! - **Erasure propagation across devices.** `ErasureReceipt` here has no
//!   `propagated_to_devices` field — a real ambient Knowledge Graph replication mechanism now
//!   exists (`hyperion_federation::kg_sync`, 2026-07-18), but this crate doesn't depend on
//!   `hyperion-federation` and an erasure (a tombstone/physical-delete) is a different operation
//!   from `kg_sync`'s own real merge (`put_node`/`link` upserts) — propagating a *deletion* across
//!   devices through that same mechanism remains a real, separate gap this crate doesn't close.
//! - ~~**`CryptoShred`'s wire-indistinguishability guarantee.**~~ `hyperion-knowledge-graph` has
//!   no node-delete operation (only edges tombstone), so `erasure::erase` overwrote a node's
//!   metadata with a tombstone-shaped placeholder rather than physically removing it — now real:
//!   `erase(CryptoShred)` calls the real
//!   `hyperion_knowledge_graph::KnowledgeGraph::delete_node`, a genuine tombstone no
//!   `get`/`query`/`traverse`/`dump` call ever surfaces again — not merely an overwritten-but-
//!   still-readable placeholder. `erase(SoftDelete)` deliberately keeps the placeholder overwrite:
//!   its own real grace-period `undo` restores through `put_node`, which could never un-tombstone
//!   a node `delete_node` had genuinely deleted. ~~Still not a byte-level deletion from the WAL's
//!   history, which no crate in this workspace performs~~ (2026-07-18) — now real too:
//!   `erase(CryptoShred)` and [`erasure::expire_lapsed_soft_deletes`] both additionally call the
//!   new `hyperion_knowledge_graph::KnowledgeGraph::purge_node_history`, which really deletes
//!   every WAL record a shredded node ever had (current head included) via a new
//!   `hyperion_storage::StorageEngine::purge_object` — not merely invisible through this
//!   workspace's own read APIs, genuinely gone from the underlying log a direct replay would
//!   otherwise still recover. A real `CryptoShred` would additionally destroy the encryption key
//!   old versions were sealed under, for a caller using `hyperion-knowledge-graph::
//!   KnowledgeGraph::open_encrypted`'s own whole-graph key — this crate's shred deletes the
//!   ciphertext bytes themselves regardless, but a whole-graph key (rather than a real per-object
//!   one) means the key itself can't be selectively destroyed without affecting every other
//!   object; that finer-grained key-wrapping design remains this crate's own separately-named
//!   "real encryption at rest, key wrapping" deferral below. Nothing here disguises an erasure's
//!   network/timing signature either (moot without a real transport anyway).
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
//! - ~~**The expired grace period's own actual shredding.**~~ (2026-07-16) — now real:
//!   [`erasure::expire_lapsed_soft_deletes`] previously only sealed the `ActionRecord` against
//!   `undo` via `RecoveryService::expire`, leaving the object itself as an
//!   overwritten-but-still-readable `"Erased"` placeholder forever — contradicting this crate's
//!   own doc comment's claim of matching `CryptoShred`'s irreversibility "from the start." It now
//!   additionally calls `hyperion_knowledge_graph::KnowledgeGraph::delete_node` on every one of
//!   the expired action's `objects_touched`, a genuine tombstone no `get`/`query`/`traverse`/
//!   `dump` call ever surfaces again — the same real primitive [33 — Rollback &
//!   Recovery](../33-rollback-recovery.md)'s own `apply_snapshot` undo-path already wires to,
//!   `GraphError::NotFound` treated as benign the same way. ~~[`erasure::expire_lapsed_soft_deletes`]
//!   itself took the same `monitor`/`token` `erase` gates on `RightsMask::WRITE`, but never
//!   checked them at all, and swept every `ActionRecord` in the whole `RecoveryService` with no
//!   Trust-Boundary scoping~~ (2026-07-16) — now real: it's `require`'d the same way `erase` is
//!   (returning `Result<Vec<ActionId>, PrivacyError>`, not a bare `Vec`), and a record is only
//!   ever eligible if every one of its `objects_touched` is genuinely visible to the caller's
//!   own token via a real `graph.get` check — reusing `hyperion-knowledge-graph`'s own real
//!   owner-based ACL directly rather than this crate inventing a second, parallel ownership
//!   concept, the same "only ever touches the caller's own objects" convention
//!   `hyperion-knowledge-graph::prune_decayed_edges` established for the identical sweep shape.
//!   Before this fix, a caller from a different Trust Boundary than the one that ran `erase`
//!   could permanently seal another boundary's own still-`Committed` grace period — stripping
//!   its undo protection — without ever being authorized to read or write its objects.
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
