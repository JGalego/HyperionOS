use serde::{Deserialize, Serialize};

/// A Semantic Object's identity — reuses `hyperion_storage::ObjectId` rather
/// than minting a parallel one, since every node *is* a storage-engine
/// object (docs/29's `semantic_objects.object_id`).
pub type NodeId = hyperion_storage::ObjectId;

/// An edge's identity. Physically the same namespace as [`NodeId`] (both are
/// just WAL record identities assigned by the same `StorageEngine`), kept as
/// a distinct type alias so call sites can't accidentally pass a node id
/// where an edge id is expected, mirroring docs/29's separate `edge_id` PK.
pub type EdgeId = hyperion_storage::ObjectId;

/// docs/09 §4 `Node.type` enum, kept open (`String`) rather than a closed
/// Rust enum — docs/29 §Data Structures notes new object types arrive via
/// [24 — Plugin Framework](../24-plugin-framework.md) Capabilities without a
/// schema migration; a closed enum here would defeat that.
pub type ObjectType = String;

/// docs/09 §4's `Node`, narrowed per this crate's top-level deferred-scope
/// list (no `content_ref`/blob, no `reasoning_provenance` chain yet — an
/// edge's own `provenance` field plus `owner`/`device_origin` cover this
/// crate's audit needs for now).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRecord {
    pub object_type: ObjectType,
    /// `None` for objects with no semantic content yet, or before Phase 3's
    /// Local AI Runtime backfills one — docs/29 §Schema: "NULL for objects
    /// with no semantic content."
    pub embedding: Option<Vec<f32>>,
    pub metadata: serde_json::Value,
    /// `TrustBoundaryId.0` of the Trust Boundary that authored the current
    /// version — docs/29 `device_origin`/`owner_id`, collapsed to one field
    /// since this simulator has no separate device/owner distinction yet.
    pub owner: u64,
    pub created_at: u64,
    pub updated_at: u64,
    /// This crate's own previously-named "no node-delete operation (only edges tombstone)" gap,
    /// closed the same way edges already are: [`crate::graph::KnowledgeGraph::delete_node`]
    /// tombstones rather than physically removing, per docs/09 §10's own "deletions are
    /// tombstones... undoable within a retention window" precedent. `#[serde(default)]` so a
    /// WAL record written before this field existed still replays as "not tombstoned" -- the
    /// exact behavior every such node already had.
    #[serde(default)]
    pub tombstone: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeOrigin {
    Explicit,
    Inferred,
}

/// docs/09 §4's `Edge`, with `version_vector` simplified to a single
/// monotonic `version` counter per triple — see the crate doc's "Multi-device
/// CRDT merge" deferral.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeRecord {
    pub subject: NodeId,
    pub predicate: String,
    pub target: NodeId,
    pub weight: f32,
    pub provenance: String,
    pub origin: EdgeOrigin,
    pub confidence: Option<f32>,
    pub owner: u64,
    pub created_at: u64,
    /// docs/09 §5.2's own previously-named decay gap: distinct from `created_at` (which never
    /// changes once an edge first exists), this is the real timestamp of the *most recent*
    /// confirmation -- a fresh [`crate::graph::KnowledgeGraph::link`] call for an already-existing
    /// edge refreshes it, exactly the "continued co-occurrence or continued similarity" event
    /// docs/09 §5.2 says keeps an inferred edge from decaying. See
    /// [`crate::decay::effective_edge_weight`] for the real decay this drives.
    #[serde(default)]
    pub last_confirmed_at: u64,
    pub tombstone: bool,
    pub version: u64,
}

/// The payload every WAL record's `metadata` field actually holds in this
/// crate — a node snapshot or an edge snapshot, tagged so replay can rebuild
/// both materialized views (adjacency + node table) from one log, per this
/// crate's doc comment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Record {
    Node(NodeRecord),
    Edge(EdgeRecord),
}

/// docs/06 §Data Structures' `RelevanceVector` is Phase 2's Context Engine
/// concern, not this crate's — [`GraphQuery`] here is docs/09 §6's
/// `graph.query` argument, deliberately narrower.
#[derive(Debug, Clone, Default)]
pub struct GraphQuery {
    pub type_filter: Option<Vec<ObjectType>>,
    pub embedding_query: Option<Vec<f32>>,
    /// Inclusive `(created_at, created_at)` bounds — docs/09 §4's temporal
    /// index, narrowed to an exact range rather than the fuzzy
    /// Gaussian-window scoring docs/09 §7's worked example performs; window
    /// densities like that belong to [05 — Intent Engine](../05-intent-engine.md),
    /// which can pre-compute `(lo, hi)` bounds from its own fuzzy prior
    /// before calling here.
    pub time_range: Option<(u64, u64)>,
    pub edge_constraint: Option<EdgeConstraint>,
    /// `0` means unbounded.
    pub limit: usize,
}

/// docs/09 §7's `EdgeConstraint(type="read-by", target=principal)` — a
/// candidate node must have an edge of `predicate` connecting it to `node`
/// (checked in either direction; see [`crate::graph::KnowledgeGraph::query`]).
#[derive(Debug, Clone)]
pub struct EdgeConstraint {
    pub predicate: String,
    pub node: NodeId,
}

/// One ranked result from [`crate::graph::KnowledgeGraph::query`].
#[derive(Debug, Clone)]
pub struct QueryHit {
    pub node_id: NodeId,
    pub node: NodeRecord,
    /// Cosine similarity to `embedding_query` when both are present; `1.0`
    /// (neutral, all candidates tie) when the query carries no embedding —
    /// see docs/09 §7 step 5's weighted re-rank, which this crate leaves to
    /// callers layering additional signals (the Context Engine's own
    /// ranker, docs/06) on top of this crate's raw similarity score.
    pub score: f32,
}

/// The result of [`crate::graph::KnowledgeGraph::traverse`] — docs/09 §6's
/// `Subgraph`. Each node carries its hop distance from the traversal's
/// start node (`0` for the start itself) — [06 — Context
/// Engine](../06-context-engine.md) §Data Structures' `RelevanceVector.
/// graph_distance` needs exactly this, so it is computed once here rather
/// than re-derived by every caller via repeated shallow traversals.
#[derive(Debug, Clone, Default)]
pub struct Subgraph {
    pub nodes: Vec<(NodeId, NodeRecord, usize)>,
    pub edges: Vec<(EdgeId, EdgeRecord)>,
}

/// What [`crate::graph::KnowledgeGraph::link`] actually did — distinguished
/// rather than collapsed into a single "ok" so the CRDT tombstone-never-
/// resurrected invariant (docs/09 §5.4) is observable by callers and tests,
/// not just an internal implementation detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkOutcome {
    /// No edge existed for this `(subject, predicate, target)` triple; a new
    /// one was created.
    Created(EdgeId),
    /// An active (non-tombstoned) edge already existed; its weight/
    /// provenance/confidence were refreshed in place.
    Updated(EdgeId),
    /// The edge was tombstoned and the caller's `observed_version` proved it
    /// had not seen that deletion — the insert was dropped rather than
    /// resurrecting a deletion it didn't know about (docs/09 §5.4).
    SuppressedByTombstone(EdgeId),
}

/// [`crate::graph::KnowledgeGraph::dump`]'s return value -- every live node and edge the caller's
/// own Trust Boundary can see, both sorted ascending by id. That sort is the whole point: `NodeId`/
/// `EdgeId` are monotonically assigned (never reused, never reordered), so two dumps of an
/// unchanged graph are byte-for-byte identical regardless of the underlying `HashMap`'s own
/// unspecified iteration order -- the property a caller diffing two dumps (e.g. before/after a
/// scenario) depends on. Tombstoned edges are omitted, matching [`crate::graph::KnowledgeGraph::
/// query`]/[`crate::graph::KnowledgeGraph::traverse`]'s own "active edges only" view -- a caller
/// comparing two dumps sees a deleted edge disappear, exactly as intended, without this crate
/// needing to represent deletion explicitly.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphSnapshot {
    pub nodes: Vec<(NodeId, NodeRecord)>,
    pub edges: Vec<(EdgeId, EdgeRecord)>,
}

/// docs/09 §6's `graph.explain(node_id | edge_id) -> ProvenanceChain`.
#[derive(Debug, Clone)]
pub enum ProvenanceChain {
    Node {
        node_id: NodeId,
        object_type: ObjectType,
        owner: u64,
        created_at: u64,
        updated_at: u64,
        incident_edges: Vec<EdgeId>,
    },
    Edge {
        edge_id: EdgeId,
        subject: NodeId,
        predicate: String,
        target: NodeId,
        origin: EdgeOrigin,
        provenance: String,
        confidence: Option<f32>,
        created_at: u64,
        tombstone: bool,
    },
}

/// Argument to `explain` — see [`ProvenanceChain`].
#[derive(Debug, Clone, Copy)]
pub enum ExplainRef {
    Node(NodeId),
    Edge(EdgeId),
}

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("no such node or edge")]
    NotFound,
    #[error("storage error: {0}")]
    Storage(#[from] hyperion_storage::StorageError),
}
