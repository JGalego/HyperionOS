use serde::{Deserialize, Serialize};

/// Identity of a Semantic Object as far as the Storage Engine is concerned —
/// distinct from `hyperion_capability::ObjectId`, which names a *kernel*
/// object (an endpoint, a region, ...). A Semantic Object and the kernel
/// objects involved in reading/writing it are different namespaces
/// entirely, per docs/28-storage-engine.md's own scoping: this engine
/// "does not know what a 'vacation' is... those are the concerns of
/// [09 — Knowledge Graph]," but it does need a stable identity to key its
/// four (here: two) materialized views by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ObjectId(pub u64);

/// One node in an object's version chain (docs/28 §Data Structures). Global
/// and monotonic in this engine, rather than per-object, which is a
/// simplification: it means version ordering is comparable across objects
/// for free, at the cost of burning version numbers for objects that never
/// touch each other — an acceptable trade at this scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct VersionId(pub u64);

/// One Object Write Transaction, durably appended to the WAL — docs/28
/// §Data Structures' `WAL record`, narrowed to this crate's current scope.
///
/// Three simplifications relative to the full spec, each noted because a
/// later session should either accept them permanently or lift them
/// deliberately, not rediscover them by surprise:
/// - `metadata` is the object's full new value, not an RFC 6902 JSON Patch
///   against the previous value — the apply phase is a plain replace, not a
///   patch-merge. Add real patch semantics when a caller needs to update
///   one field without resending the whole object.
/// - There is no `blob_hash` / chunk manifest field: the content-addressed
///   blob store, its BLAKE3 hashing, and its envelope encryption
///   (docs/28 §Security Considerations) are not implemented yet — this
///   crate persists metadata only.
/// - There is no `edge_deltas` or `embedding` field: the graph index and
///   vector index are [09 — Knowledge Graph](../09-knowledge-graph.md)'s
///   concern, layered on top of this engine in a later session, not this
///   one.
/// - `actor_origin` records *which Trust Boundary* wrote this record (for
///   audit/provenance), not a live `CapabilityToken` — a token is
///   unforgeable and monitor-local by design (see hyperion-capability's
///   crate docs) and was never meant to survive a restart; the capability
///   check itself happens before a record is ever constructed, in
///   [`crate::StorageEngine::put_object`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalRecord {
    pub object_id: ObjectId,
    pub prev_version: Option<VersionId>,
    pub new_version: VersionId,
    pub metadata: serde_json::Value,
    pub actor_origin: u64,
}

/// One entry in an object's version chain — docs/28 §Data Structures'
/// `Version record`, narrowed the same way `WalRecord` is (a metadata
/// snapshot, not a blob hash / diff).
#[derive(Debug, Clone)]
pub struct VersionRecord {
    pub object_id: ObjectId,
    pub parent_version: Option<VersionId>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("no such object or version")]
    NotFound,
    /// docs/28 §Algorithms' "Write path" step 2: "the record includes the
    /// version the caller believed was current; if another writer has since
    /// advanced the object's version pointer, the append is rejected and
    /// the caller retries against the new head (a compare-and-swap on the
    /// version pointer, not a lock)."
    #[error("concurrent write conflict on {object_id:?}: expected {expected:?}, found {found:?}")]
    ConcurrentWriteConflict {
        object_id: ObjectId,
        expected: Option<VersionId>,
        found: Option<VersionId>,
    },
    #[error("WAL I/O error: {0}")]
    Wal(#[from] std::io::Error),
    #[error("WAL record encoding error: {0}")]
    Encoding(#[from] serde_json::Error),
}
