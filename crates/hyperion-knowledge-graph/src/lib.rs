//! Hyperion L3 Knowledge Graph — Phase 2, second slice.
//!
//! Implements the concrete schema of docs/29-database-schema.md (nodes and
//! edges as the two tables of record, `object_versions`-equivalent history
//! riding on `hyperion-storage`'s existing version chain, `collections` as a
//! thin view expressible in terms of the other two) and the graph/embedding
//! model of docs/09-knowledge-graph.md (typed weighted edges, vector
//! similarity, bidirectional traversal, CRDT-style tombstoned edge deletion)
//! on top of `hyperion-storage`'s WAL — never as a second, independently
//! committed store. Every node and every edge is written through
//! [`hyperion_storage::StorageEngine::put_object`], so "the four stores are
//! materialized views of the WAL" (docs/28 §Architecture) continues to hold:
//! this crate's [`KnowledgeGraph::open`] rebuilds its entire adjacency and
//! vector index by replaying the same WAL `hyperion-storage` already
//! replays, via [`hyperion_storage::Wal::replay`], never from separately
//! persisted state.
//!
//! Two on-disk tables become one physical namespace here: a node and an edge
//! are both just a [`hyperion_storage::ObjectId`]-keyed WAL record whose
//! `metadata` payload is a [`types::Record::Node`] or [`types::Record::Edge`]
//! — docs/29's separate `edge_id`/`object_id` primary-key sequences are
//! simplified to one shared counter, since nothing here depends on the two
//! sequences being independent.
//!
//! [`providers::capability_for_topic`] (2026-07-16, docs/998-roadmap.md's Resourceful pillar)
//! is a real (topic -> capability_id) lookup: a plugin's own
//! `hyperion_plugin_framework::Contribution::KnowledgeProvider` entries are searched for a
//! topic this crate has no local knowledge of, and a real caller uses the match (if any) to
//! decide which installed Capability to invoke — never a second, parallel dispatch path; the
//! matched capability still goes through the exact same invocation/consent machinery every
//! other Capability already does.
//!
//! Explicitly deferred, and why, matching the scoping this workspace already
//! uses (see `hyperion-storage`'s crate doc for the same pattern):
//!
//! - **Real embeddings.** [22 — Local AI Runtime](../22-local-ai-runtime.md)
//!   (docs/09 §5.1) does not exist until Phase 3; callers of this crate pass
//!   a pre-computed `Vec<f32>` (or `None`), and no embedding is ever computed
//!   in-crate. The vector index is brute-force cosine similarity, not an
//!   HNSW/ANN structure — adequate for a hosted simulator's object counts,
//!   explicitly not the docs/09 §11 performance target at scale.
//! - **`semantically-similar-to` inferred edges, and decay for either
//!   kind** (docs/09 §5.2). The `co-occurs-with` half is now real:
//!   `hyperion-memory::MemoryEngine::run_co_occurrence_pass` (that crate
//!   is real as of Phase 3) submits a real `hyperion-scheduler`
//!   `BatchDistributable` task and links every pair of objects a real
//!   `MemoryRecord.provenance` names together. `semantically-similar-to`
//!   still needs real embeddings this workspace doesn't have, and
//!   neither kind of inferred edge decays yet (weight is reset to a
//!   fixed value each pass, not accumulated or aged).
//! - ~~**Per-object ACL enforcement.**~~ Now real for [`graph::KnowledgeGraph::query`]/
//!   [`graph::KnowledgeGraph::traverse`] — Phase 8's hardening pass
//!   (docs/41-implementation-phases.md's own framing: "minimal versions... already exist from
//!   earlier phases; Phase 8 is where they reach production rigor") landed everywhere else in
//!   this workspace but had never actually reached this crate's own two read paths that fan out
//!   over many objects at once. Every public call here is still capability-gated the coarse way
//!   (a single READ/WRITE rights check per call, same as `hyperion-storage`'s own
//!   `get_object`/`put_object`), but `query`/`traverse` now also filter by the already-recorded
//!   `owner` field, per docs/09 §8's "capability-checked at every hop": a candidate — or, for
//!   `traverse`, a whole subtree reachable only through one — outside the caller's own Trust
//!   Boundary is excluded entirely, never included and merely down-ranked, mirroring
//!   `hyperion-context::engine`'s own downstream filter of the same shape (confirmed safe to add
//!   here too: every real caller across the workspace already operates strictly within its own
//!   token's boundary). `device_origin`-based filtering (a finer axis than plain `owner`) remains
//!   unimplemented, as does docs/29's richer per-row `acl` JSONB — this closes the coarser,
//!   `owner`-only half of the gap this bullet named.
//! - **Multi-device CRDT merge.** Edge version tracking here is a single
//!   monotonic counter per `(subject, predicate, target)` triple, enough to
//!   prove and test the core invariant docs/09 §5.4 cares about most — "a
//!   deletion is never silently undone by a late-arriving insertion from a
//!   device that hadn't seen it yet" — without the full multi-replica
//!   `version_vector` map, which only matters once more than one device
//!   exists ([21 — Distributed Execution](../21-distributed-execution.md),
//!   Phase 7).
//! - **Sharding/partitioning** (docs/29 §Sharding and Partitioning) is a
//!   multi-device/multi-tenant concern; a hosted simulator has exactly one
//!   shard.

mod graph;
mod index;
mod providers;
mod types;

pub use graph::KnowledgeGraph;
pub use providers::{capabilities_for_topic, capability_for_topic};
// Re-exported the same way `NodeId`/`EdgeId` already alias `hyperion_storage::ObjectId` in
// types.rs — a caller of `KnowledgeGraph::current_version`/`get_at_version` needs to name this
// type without taking its own direct dependency on `hyperion-storage`.
pub use hyperion_storage::VersionId;
pub use types::{
    EdgeConstraint, EdgeId, EdgeOrigin, EdgeRecord, ExplainRef, GraphError, GraphQuery,
    GraphSnapshot, LinkOutcome, NodeId, NodeRecord, ProvenanceChain, QueryHit, Subgraph,
};
