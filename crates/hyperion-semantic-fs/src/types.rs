use hyperion_knowledge_graph::NodeId;

/// docs/10 §Data Structures' `QuerySpec` — the structured form a query
/// (native or synthesized-path) compiles into before resolution.
#[derive(Debug, Clone, Default)]
pub struct QuerySpec {
    pub anchor: Option<NodeId>,
    pub hop_bound: usize,
    /// Edge predicates to follow during the relational traversal leg —
    /// docs/10 §Algorithms' `predicate_filter`.
    pub predicate_filter: Option<Vec<String>>,
    /// Object-type filter for the vector-similarity leg.
    pub type_filter: Option<Vec<String>>,
    pub embedding: Option<Vec<f32>>,
    pub k: usize,
    pub ttl_secs: u64,
}

/// docs/10 §Data Structures' `VirtualFolder`. Immutable once created —
/// see this crate's doc comment on how that gives `snapshot_token`
/// stability for free.
#[derive(Debug, Clone)]
pub struct VirtualFolder {
    pub folder_id: u64,
    pub query_spec: QuerySpec,
    pub member_object_ids: Vec<NodeId>,
    pub materialized_at: u64,
    pub ttl_secs: u64,
    pub snapshot_token: u64,
}

/// docs/10 §Data Structures' `Collection` — a real Knowledge Graph node of
/// `object_type = "collection"`; this struct is just a convenience view
/// over it, not a separate stored entity.
#[derive(Debug, Clone)]
pub struct Collection {
    pub collection_id: NodeId,
    pub name: String,
}

/// docs/10 §Interfaces' `[DirEntry]` — one synthesized path per materialized
/// VirtualFolder member.
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub path: String,
    pub object_id: NodeId,
}
