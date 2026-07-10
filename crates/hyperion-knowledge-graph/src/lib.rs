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
//! Explicitly deferred, and why, matching the scoping this workspace already
//! uses (see `hyperion-storage`'s crate doc for the same pattern):
//!
//! - **Real embeddings.** [22 — Local AI Runtime](../22-local-ai-runtime.md)
//!   (docs/09 §5.1) does not exist until Phase 3; callers of this crate pass
//!   a pre-computed `Vec<f32>` (or `None`), and no embedding is ever computed
//!   in-crate. The vector index is brute-force cosine similarity, not an
//!   HNSW/ANN structure — adequate for a hosted simulator's object counts,
//!   explicitly not the docs/09 §11 performance target at scale.
//! - **Inferred-edge background job** (docs/09 §5.2:
//!   `semantically-similar-to`, `discussed-in`/`co-occurs-with`, and their
//!   decay) needs both a scheduler-driven background job
//!   ([04 — Scheduler](../04-scheduler.md), already real) and a real Memory
//!   Engine (Phase 3, not yet real) to source co-occurrence from — deferred
//!   whole to Phase 3.
//! - **Per-object ACL enforcement.** Every public call here is
//!   capability-gated the same way `hyperion-storage` already gates
//!   `get_object`/`put_object` — a single coarse READ/WRITE rights check per
//!   call via `hyperion_capability::CapabilityMonitor`. docs/29's per-row
//!   `acl` JSONB and docs/09 §8's "capability-checked at every hop" describe
//!   a *finer* per-object/per-hop authorization model; this crate records
//!   `owner`/`device_origin` on every node for audit and later enforcement
//!   but does not yet filter query/traversal results by them. That
//!   enforcement is explicitly Phase 8's hardening pass
//!   (docs/41-implementation-phases.md's own framing: "minimal versions...
//!   already exist from earlier phases; Phase 8 is where they reach
//!   production rigor"), not a Phase 2 exit criterion.
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
mod types;

pub use graph::KnowledgeGraph;
pub use types::{
    EdgeConstraint, EdgeId, EdgeOrigin, EdgeRecord, ExplainRef, GraphError, GraphQuery,
    LinkOutcome, NodeId, NodeRecord, ProvenanceChain, QueryHit, Subgraph,
};
