//! Hyperion L3 Knowledge Graph — Phase 2, second slice.
//!
//! Implements the concrete schema of docs/29-database-schema.md (nodes and
//! edges as the two tables of record, `object_versions`-equivalent history
//! riding on `hyperion-storage`'s existing version chain, `collections` as a
//! thin view expressible in terms of the other two) and the graph/embedding
//! model of docs/09-knowledge-graph.md (typed weighted edges, vector
//! similarity, bidirectional traversal, CRDT-style tombstoned node/edge deletion)
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
//! - **`semantically-similar-to` inferred edges** (docs/09 §5.2) still need real embeddings this
//!   workspace doesn't have. The `co-occurs-with` half is real:
//!   `hyperion-memory::MemoryEngine::run_co_occurrence_pass` (that crate
//!   is real as of Phase 3) submits a real `hyperion-scheduler`
//!   `BatchDistributable` task and links every pair of objects a real
//!   `MemoryRecord.provenance` names together. ~~Decay for either kind~~ — now real:
//!   [`decay::effective_edge_weight`] closes "weight is reset to a fixed value each pass, not
//!   accumulated or aged" for real, on demand — an [`types::EdgeOrigin::Inferred`] edge's real
//!   weight shrinks with real elapsed time since [`types::EdgeRecord::last_confirmed_at`] (the
//!   same recency-weighted mechanism docs/09 §5.2 names, `hyperion-memory::decay::decay_score`'s
//!   own tau); an [`types::EdgeOrigin::Explicit`] edge never decays at all — "a hypothesis is
//!   allowed to fade," an explicit fact is not. A pure, recompute-from-scratch function (mirrors
//!   `decay_score`'s own shape), not a batch job overwriting `weight` in place.
//!   ~~Pruning below a confidence threshold~~ (2026-07-16) — now real too:
//!   [`graph::KnowledgeGraph::prune_decayed_edges`] is docs/28's own paired "Garbage collection /
//!   compaction" gap for this crate ("inferred edges below a confidence threshold... are pruned
//!   ... explicit edges... are never auto-pruned") — every non-tombstoned `Inferred` edge whose
//!   real `effective_edge_weight` has fallen below a caller-chosen threshold is tombstoned for
//!   real via the existing [`graph::KnowledgeGraph::unlink`], never an `Explicit` edge regardless
//!   of threshold. Named simplification: no separate provenance-TTL field exists on
//!   `EdgeRecord` distinct from `last_confirmed_at`'s own tau-decay, so the confidence check
//!   alone is this sweep's one real mechanism.
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
//!   `owner`-only half of the gap this bullet named. ~~This crate's own `traverse` doc comment
//!   claimed [`graph::KnowledgeGraph::get`] already gave the same owner-checked shape it does —
//!   it didn't~~ (2026-07-16): [`graph::KnowledgeGraph::get`]/[`graph::KnowledgeGraph::get_at_version`]/
//!   [`graph::KnowledgeGraph::delete_node`]/[`graph::KnowledgeGraph::unlink`]/
//!   [`graph::KnowledgeGraph::explain`] now all real-check `owner` too, closing the same
//!   contradiction for every single-object accessor `query`/`traverse`/`dump` already had fixed.
//!   [`graph::KnowledgeGraph::put_node`] had a related, more severe bug this same pass closed:
//!   updating an *existing* node always overwrote its `owner` to the caller's own boundary,
//!   letting any caller with a live WRITE-rights token silently steal a foreign-boundary node —
//!   and, worse, use that theft to bypass every owner check just landed. An update to a node the
//!   caller doesn't already own is now rejected (`GraphError::NotFound`, never revealing the
//!   node's existence), mirroring `link`'s own edge-owner-preservation path, which never had
//!   this bug (edges already preserve `existing.owner` verbatim across an update).
//! - ~~**Node deletion.**~~ Now real: `hyperion-recovery`/`hyperion-privacy`'s own previously-named
//!   "no node-delete operation (only edges tombstone)" gap. [`graph::KnowledgeGraph::delete_node`]
//!   tombstones a node exactly the way [`graph::KnowledgeGraph::unlink`] already tombstones an
//!   edge; [`graph::KnowledgeGraph::get`]/[`graph::KnowledgeGraph::query`]/
//!   [`graph::KnowledgeGraph::traverse`]/[`graph::KnowledgeGraph::dump`] all now treat a
//!   tombstoned node as genuinely gone, and a plain [`graph::KnowledgeGraph::put_node`] update
//!   never silently resurrects one — the same "an insert never revives a deliberate deletion"
//!   invariant edges already had. This closes only the KG-side primitive itself; neither
//!   `hyperion-privacy::erasure::erase` nor `hyperion-recovery` calls it yet, so `CryptoShred`
//!   still overwrites metadata with a tombstone-shaped placeholder rather than using this real
//!   one, and "un-creating a freshly created object" via undo remains separately unimplemented —
//!   both are real, separate follow-up wiring this bullet does not itself close.
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

mod decay;
mod graph;
mod index;
mod providers;
mod types;

pub use decay::{effective_edge_weight, DEFAULT_INFERRED_EDGE_TAU_SECS};
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
