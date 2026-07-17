//! Hyperion L2/L3 storage engine — Phase 2 kickoff.
//!
//! Implements the architectural core of docs/28-storage-engine.md: a
//! write-ahead log as the sole atomicity boundary, with every other store
//! a rebuildable materialized view of it, so "partially written Semantic
//! Object" is structurally impossible rather than merely rare. Every call
//! is capability-gated through `hyperion-capability`, consistent with every
//! other crate in this workspace and with docs/28 §Security Considerations'
//! "every call... carries a capability token... re-evaluated on every
//! `get_object`... never cached across a Trust Boundary."
//!
//! This is a deliberately narrow first slice of a five-subsystem phase
//! (docs/41-implementation-phases.md's Phase 2 also covers the concrete
//! schema in 29, the Knowledge Graph in 09, and the Context Engine in
//! 06/07 — none of which exist yet). Scoped out of *this* crate, and why:
//!
//! - **Content-addressed blob store** (BLAKE3 hashing, content-defined
//!   chunking, envelope encryption) — a substantial subsystem in its own
//!   right; this engine currently persists metadata only.
//! - **Graph index and vector index** — [09 — Knowledge
//!   Graph](../09-knowledge-graph.md)'s concern, meant to be layered on top
//!   of this engine's WAL, not duplicated inside it.
//! - **Sync/replication** (Merkle-diff, CRDT merge across devices) — this
//!   is single-device only; multi-device sync is
//!   [21 — Distributed Execution](../21-distributed-execution.md)'s
//!   concern (Phase 7).
//! - ~~**Garbage collection / compaction**~~ (2026-07-16) — the version-retention slice is now
//!   real: [`engine::StorageEngine::compact`] collapses every object's version chain
//!   unconditionally to its current head (a real, WAL-rewriting sweep, not just an in-memory
//!   prune — a real restart must not resurrect history it already dropped). Named
//!   simplification: docs/28's own fuller design tiers retention across N versions/T days into
//!   periodic snapshots; this crate has no timestamp on a version record to key a time-based tier
//!   by, so every object collapses to one head uniformly, the same "one real, general mechanism,
//!   not retention *classes*" shape this session's own `hyperion-recovery`/`hyperion-privacy`
//!   compaction/expiry sweeps already established. The blob-refcount GC, inferred-edge pruning,
//!   and ANN index rebuild docs/28 also names remain genuinely out of scope — none of those
//!   subsystems (blob store, Knowledge Graph inferred edges, vector index) exist in this crate.
//!
//! What *is* implemented and tested: the WAL as commit boundary, optimistic
//! concurrency via compare-and-swap on the version pointer, full
//! crash-consistent recovery by replay, and real version-chain compaction — the properties
//! docs/28 §Testing Strategy calls out first.
//!
//! ~~**Encryption at rest**~~ (2026-07-17, [16 — Privacy Architecture](../16-privacy-architecture.md)
//! Phase 8's own CryptoShred prerequisite) — now real and opt-in: [`engine::StorageEngine::
//! open_encrypted`]/[`wal::Wal::open_for_append_encrypted`]/[`wal::Wal::replay_encrypted`] seal
//! each individual WAL record under its own fresh nonce via `hyperion_crypto::SealingKey` (never
//! one whole-file reseal per append), keyed by a caller-supplied 32-byte key -- typically
//! `hyperion_crypto::Keystore::derive_key`, the same device-bound, no-new-passphrase pattern
//! `hyperion-crypto::secret_store::SecretStore` already established. The plain, unencrypted
//! `open`/`open_for_append`/`replay` path is unchanged and still every existing caller's default;
//! this is additive, not a breaking format change.

mod engine;
mod types;
mod wal;

pub use engine::StorageEngine;
pub use types::{ObjectId, StorageError, VersionId, VersionRecord, WalRecord};
pub use wal::Wal;
