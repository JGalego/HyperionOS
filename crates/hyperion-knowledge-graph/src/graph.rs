use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_events::{EventBus, EventPayload, SubjectId, Topic, TopicKind};
use hyperion_storage::{StorageEngine, VersionId};

use crate::index::GraphIndex;
use crate::types::{
    EdgeConstraint, EdgeId, EdgeOrigin, EdgeRecord, ExplainRef, GraphError, GraphQuery,
    GraphSnapshot, LinkOutcome, NodeId, NodeRecord, ObjectType, ProvenanceChain, QueryHit, Record,
    Subgraph,
};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

/// docs/09 — Knowledge Graph, layered on `hyperion-storage`'s WAL exactly as
/// this crate's doc comment describes. See there for the full list of what
/// is deliberately not implemented yet.
pub struct KnowledgeGraph {
    storage: StorageEngine,
    index: Mutex<GraphIndex>,
    /// docs/09's own object-changed notification, cited as [31 — Event
    /// System](../31-event-system.md)'s own motivating example ("Knowledge
    /// Graph object-changed notifications"): real, optional (the same
    /// `Option<Arc<...>>` shape this workspace uses for every other
    /// optional backend) once [`Self::with_events`] wires it.
    events: Option<Arc<EventBus>>,
}

/// Domain-separates this graph's derived WAL encryption key from any other future caller of
/// [`hyperion_crypto::Keystore::derive_key`] -- see [`KnowledgeGraph::open_encrypted`].
const WAL_KEY_DERIVATION_CONTEXT: &str = "hyperion.knowledge-graph.wal.v1";

impl KnowledgeGraph {
    /// Opens (or creates) the graph at `path`. Rebuilds the adjacency/vector
    /// index by replaying the same WAL `hyperion-storage` itself replays —
    /// no separately persisted index.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, GraphError> {
        Self::open_impl(path, None)
    }

    /// As [`Self::open`], but the underlying WAL is real, per-record encrypted at rest (see
    /// `hyperion_storage::Wal`'s own doc comment) under a key derived from `device_key`'s own
    /// identity via [`hyperion_crypto::Keystore::derive_key`] -- no new passphrase or
    /// key-management UX, matching `hyperion_crypto::SecretStore`'s own established pattern. A
    /// caller opening the same graph later with a *different* `device_key` recovers an empty
    /// graph, never wrong or garbage data -- the same fail-closed behavior
    /// `StorageEngine::open_encrypted` itself already guarantees.
    pub fn open_encrypted(
        path: impl AsRef<Path>,
        device_key: &hyperion_crypto::Keystore,
    ) -> Result<Self, GraphError> {
        let key = device_key.derive_key(WAL_KEY_DERIVATION_CONTEXT);
        Self::open_impl(path, Some(key))
    }

    fn open_impl(path: impl AsRef<Path>, key: Option<[u8; 32]>) -> Result<Self, GraphError> {
        let path: PathBuf = path.as_ref().to_path_buf();
        let storage = match key {
            Some(key) => StorageEngine::open_encrypted(&path, key)?,
            None => StorageEngine::open(&path)?,
        };
        let index = match key {
            Some(key) => GraphIndex::rebuild_encrypted(&path, key)?,
            None => GraphIndex::rebuild(&path)?,
        };
        Ok(KnowledgeGraph {
            storage,
            index: Mutex::new(index),
            events: None,
        })
    }

    /// Opts this graph into docs/31-event-system.md's real broadcast bus: every real write
    /// ([`Self::put_node`]/[`Self::link`]/[`Self::unlink`]/[`Self::delete_node`]) publishes a
    /// real `ObjectChanged` event under the written record's own Trust-Boundary owner —
    /// `hyperion-semantic-fs`'s own live `VirtualFolder` invalidation is the first real
    /// consumer, but this is a general-purpose capability, not scoped to that one caller.
    pub fn with_events(mut self, events: Arc<EventBus>) -> Self {
        self.events = Some(events);
        self
    }

    /// `ancestors` names every other object id this change is also relevant to — critically, for
    /// an edge, its own `subject`/`target` node ids (see `Self::link`/`Self::unlink`'s call
    /// sites). An edge's own id lives in the same numeric namespace as node ids but is otherwise
    /// meaningless to a subscriber that only tracks node ids (e.g. `hyperion-semantic-fs`'s
    /// `VirtualFolder` membership) — without this, "an edge touching node X changed" would be
    /// unrecoverable from the event alone, only "some id Y (which happens to be an edge) changed."
    fn publish_changed(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        subject_id: u64,
        owner: u64,
        ancestors: Vec<u64>,
    ) {
        let Some(bus) = &self.events else {
            return;
        };
        let topic = Topic::new(
            TopicKind::ObjectChanged,
            SubjectId::Object(subject_id),
            "kg.object_changed.v1",
        );
        let payload = EventPayload::Inline(serde_json::json!({
            "object_id": subject_id,
            "changed_at": now(),
        }));
        if let Err(e) = bus.publish(
            monitor,
            token,
            TrustBoundaryId(owner),
            topic,
            payload,
            ancestors,
        ) {
            eprintln!("hyperion-knowledge-graph: failed to publish object-changed event: {e}");
        }
    }

    fn require(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        rights: RightsMask,
    ) -> Result<(), GraphError> {
        monitor
            .check_rights_ok_result(token, rights)
            .map_err(|_| GraphError::Unauthorized)
    }

    /// `graph.link` — docs/09 §6. Creates, refreshes, or (per docs/09 §5.4)
    /// silently drops an insert that would resurrect an unseen tombstone.
    /// `observed_version` is the version of this triple the caller last
    /// observed, if any — `None` means "no prior knowledge of this triple,"
    /// which is the case a blind concurrent insert must not be allowed to
    /// resurrect a tombstone against. Owner-checked (2026-07-16) on both the plain-update and
    /// tombstone-resurrection paths, the same real gap [`Self::put_node`]'s own doc comment
    /// names: an existing edge can only ever be updated, or a tombstoned one resurrected, by the
    /// same Trust Boundary that owns it — `GraphError::NotFound` otherwise, never revealing the
    /// edge's existence to a foreign caller.
    #[allow(clippy::too_many_arguments)]
    pub fn link(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        subject: NodeId,
        predicate: &str,
        target: NodeId,
        weight: f32,
        origin: EdgeOrigin,
        confidence: Option<f32>,
        provenance: &str,
        observed_version: Option<u64>,
    ) -> Result<LinkOutcome, GraphError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let mut index = self.index.lock().unwrap();
        let key = (subject, predicate.to_string(), target);
        let existing = index
            .edge_identity
            .get(&key)
            .copied()
            .and_then(|id| index.edges.get(&id).cloned().map(|e| (id, e)));

        let caller_boundary = token.origin().0;
        let (id_arg, expected_version, created_at, owner, is_update) = match &existing {
            Some((id, existing)) if existing.tombstone => {
                // Owner-checked (2026-07-16), before the tombstone short-circuit below, the same
                // ordering `unlink`/`delete_node` already use: a foreign-boundary caller gets
                // `NotFound`, never `SuppressedByTombstone`, which would leak that a tombstoned
                // edge with this exact identity exists under some other owner.
                if existing.owner != caller_boundary {
                    return Err(GraphError::NotFound);
                }
                let seen = observed_version.is_some_and(|v| v >= existing.version);
                if !seen {
                    return Ok(LinkOutcome::SuppressedByTombstone(*id));
                }
                (
                    Some(*id),
                    self.storage.current_version(*id),
                    existing.created_at,
                    caller_boundary,
                    false,
                )
            }
            Some((id, existing)) => {
                // Owner-checked (2026-07-16): this crate's own `put_node` doc comment claimed
                // `link`'s update path was already safe because it "preserves `existing.owner`
                // unconditionally" -- preserving the *value* isn't the same as *checking* it.
                // Before this, any caller with a plain WRITE-rights token from a different Trust
                // Boundary could silently mutate another boundary's edge (`weight`/`confidence`/
                // `provenance`/`origin`), exactly the "misattribute ownership" concern docs/09 §8
                // names.
                if existing.owner != caller_boundary {
                    return Err(GraphError::NotFound);
                }
                (
                    Some(*id),
                    self.storage.current_version(*id),
                    existing.created_at,
                    existing.owner,
                    true,
                )
            }
            None => (None, None, now(), caller_boundary, false),
        };

        let record = EdgeRecord {
            subject,
            predicate: predicate.to_string(),
            target,
            weight,
            provenance: provenance.to_string(),
            origin,
            confidence,
            owner,
            created_at,
            // Real per docs/09 §5.2: every `link` call -- fresh or reconfirming -- is a real
            // "continued co-occurrence or continued similarity" event, so this always advances,
            // unlike `created_at` above (which never changes once an edge first exists).
            last_confirmed_at: now(),
            tombstone: false,
            version: existing.as_ref().map_or(0, |(_, e)| e.version + 1),
        };
        let payload = serde_json::to_value(Record::Edge(record.clone())).unwrap();
        let (assigned_id, _) =
            self.storage
                .put_object(monitor, token, id_arg, expected_version, payload)?;
        index.apply(assigned_id, Record::Edge(record));
        self.publish_changed(
            monitor,
            token,
            assigned_id.0,
            owner,
            vec![subject.0, target.0],
        );

        Ok(if is_update {
            LinkOutcome::Updated(assigned_id)
        } else {
            LinkOutcome::Created(assigned_id)
        })
    }

    /// `graph.unlink` — docs/09 §6. Tombstones rather than physically
    /// removing (docs/09 §10: "edge deletions are tombstones... undoable
    /// within a retention window"). A no-op if already tombstoned. Owner-checked (2026-07-16)
    /// the same way [`Self::get`]/[`Self::delete_node`] now are, and *before* the tombstone
    /// short-circuit: a caller outside the edge's own Trust Boundary gets `NotFound`, never
    /// learning whether the edge even exists.
    pub fn unlink(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        edge_id: EdgeId,
    ) -> Result<(), GraphError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let caller_boundary = token.origin().0;

        let mut index = self.index.lock().unwrap();
        let existing = index
            .edges
            .get(&edge_id)
            .filter(|e| e.owner == caller_boundary)
            .ok_or(GraphError::NotFound)?
            .clone();
        if existing.tombstone {
            return Ok(());
        }

        let record = EdgeRecord {
            tombstone: true,
            version: existing.version + 1,
            ..existing
        };
        let payload = serde_json::to_value(Record::Edge(record.clone())).unwrap();
        self.storage.put_object(
            monitor,
            token,
            Some(edge_id),
            self.storage.current_version(edge_id),
            payload,
        )?;
        index.apply(edge_id, Record::Edge(record));
        self.publish_changed(
            monitor,
            token,
            edge_id.0,
            caller_boundary,
            vec![existing.subject.0, existing.target.0],
        );
        Ok(())
    }

    /// docs/28 §"Garbage collection / compaction"'s own named gap for this crate: "inferred
    /// edges below a confidence threshold and past their provenance TTL are pruned... explicit
    /// edges... are never auto-pruned." [`crate::decay::effective_edge_weight`] was the real,
    /// on-demand *read* half of this; this is the real *sweep* — every non-tombstoned
    /// [`EdgeOrigin::Inferred`] edge whose current [`crate::decay::effective_edge_weight`] has
    /// fallen below `threshold` is tombstoned for real via [`Self::unlink`] (the same real,
    /// WAL-backed, undoable-within-a-retention-window tombstone every other deletion in this
    /// crate already uses) — an [`EdgeOrigin::Explicit`] edge is never even considered,
    /// regardless of `threshold` or `now`, matching docs/28's own explicit carve-out. Named
    /// simplification: `EdgeRecord` has no separate provenance-TTL field distinct from
    /// `last_confirmed_at`'s own tau-decay, so the confidence-threshold check alone is this
    /// sweep's one real mechanism — the same "one real, general mechanism, not two separate
    /// ones" shape this session's other compaction sweeps already established, rather than
    /// inventing a second, redundant TTL clock this crate's own data model doesn't carry.
    /// Returns every `EdgeId` this call really pruned, so a caller can log or audit exactly what
    /// a sweep did — never a silent bulk operation with no visible effect. Scoped to the caller's
    /// own Trust Boundary (2026-07-16), the same owner check [`Self::unlink`] now performs on
    /// every single edge it tombstones: a sweep only ever considers edges the caller owns, never
    /// erroring out partway through (nor silently pruning cross-boundary) if some other
    /// boundary's inferred edges also happen to be decayed.
    pub fn prune_decayed_edges(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        threshold: f32,
        now: u64,
    ) -> Result<Vec<EdgeId>, GraphError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let caller_boundary = token.origin().0;

        let to_prune: Vec<EdgeId> = {
            let index = self.index.lock().unwrap();
            index
                .edges
                .iter()
                .filter(|(_, edge)| {
                    !edge.tombstone
                        && edge.owner == caller_boundary
                        && edge.origin == EdgeOrigin::Inferred
                        && crate::decay::effective_edge_weight(edge, now) < threshold
                })
                .map(|(&id, _)| id)
                .collect()
        };

        for &id in &to_prune {
            self.unlink(monitor, token, id)?;
        }

        Ok(to_prune)
    }

    /// `graph.link` for a fresh node — docs/09 §6 has no separate "create
    /// node" verb (nodes are implicitly created by writing a Semantic
    /// Object elsewhere in the system); this crate exposes it explicitly
    /// since it owns the node table.
    pub fn put_node(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        node_id: Option<NodeId>,
        object_type: impl Into<ObjectType>,
        embedding: Option<Vec<f32>>,
        metadata: serde_json::Value,
    ) -> Result<NodeId, GraphError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let caller_boundary = token.origin().0;

        let mut index = self.index.lock().unwrap();
        let expected_version = node_id.and_then(|id| self.storage.current_version(id));
        let existing = node_id.and_then(|id| index.nodes.get(&id));
        // Owner-checked (2026-07-16): an update to an existing node this caller doesn't own is
        // rejected outright -- previously this unconditionally set `owner: token.origin().0` on
        // every write, silently reassigning a foreign-boundary node to whichever caller last
        // updated it (and, worse, making every owner check this crate now performs elsewhere
        // trivially bypassable: steal ownership here, then freely `get`/`delete_node` as the new
        // "owner"). `link`'s own analogous update path already preserves `existing.owner`
        // unconditionally; this closes the same gap for nodes.
        if let Some(existing_node) = existing {
            if existing_node.owner != caller_boundary {
                return Err(GraphError::NotFound);
            }
        }
        let created_at = existing.map(|n| n.created_at).unwrap_or_else(now);
        // A plain `put_node` update never resurrects a tombstoned node -- the same "an insert
        // never silently revives what was deliberately deleted" precedent `link`'s own tombstone
        // handling already establishes for edges. `Self::delete_node` is the one real way a
        // tombstone is ever set; nothing here clears it.
        let tombstone = existing.is_some_and(|n| n.tombstone);

        let record = NodeRecord {
            object_type: object_type.into(),
            embedding,
            metadata,
            owner: existing.map_or(caller_boundary, |n| n.owner),
            created_at,
            updated_at: now(),
            tombstone,
        };
        let payload = serde_json::to_value(Record::Node(record.clone())).unwrap();
        let (assigned_id, _) =
            self.storage
                .put_object(monitor, token, node_id, expected_version, payload)?;
        index.apply(assigned_id, Record::Node(record));
        self.publish_changed(monitor, token, assigned_id.0, caller_boundary, Vec::new());
        Ok(assigned_id)
    }

    /// `graph.get` — docs/09 §6. docs/09 §8's "capability-checked at every hop, not merely at
    /// the query boundary" now really holds here too (2026-07-16): a node outside the caller's
    /// own Trust Boundary is `NotFound`, the same "never reveal existence of what you can't see"
    /// [`Self::query`]/[`Self::traverse`]/[`Self::dump`] already give — this crate's own
    /// `traverse` doc comment already claimed this exact shape for `get`; now it's true rather
    /// than aspirational.
    pub fn get(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        node_id: NodeId,
    ) -> Result<NodeRecord, GraphError> {
        self.require(monitor, token, RightsMask::READ)?;
        let caller_boundary = token.origin().0;
        let index = self.index.lock().unwrap();
        index
            .nodes
            .get(&node_id)
            .filter(|n| !n.tombstone && n.owner == caller_boundary)
            .cloned()
            .ok_or(GraphError::NotFound)
    }

    /// `graph.delete_node` — this crate's own previously-named "no node-delete operation (only
    /// edges tombstone)" gap, closed the same way [`Self::unlink`] already tombstones an edge:
    /// per docs/09 §10's own "deletions are tombstones... undoable within a retention window"
    /// precedent, applied to nodes for the first time. [`Self::get`]/[`Self::query`]/
    /// [`Self::traverse`]/[`Self::dump`] all exclude a tombstoned node exactly as they already
    /// exclude a tombstoned edge — a real `NotFound`/omission for every caller, not merely
    /// hidden metadata. A no-op if already tombstoned. Owner-checked (2026-07-16) the same way
    /// [`Self::get`] now is, and *before* the tombstone short-circuit: a caller outside the
    /// node's own Trust Boundary gets `NotFound`, never learning whether the node even exists,
    /// let alone whether it's already deleted.
    pub fn delete_node(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        node_id: NodeId,
    ) -> Result<(), GraphError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let caller_boundary = token.origin().0;

        let mut index = self.index.lock().unwrap();
        let existing = index
            .nodes
            .get(&node_id)
            .filter(|n| n.owner == caller_boundary)
            .ok_or(GraphError::NotFound)?
            .clone();
        if existing.tombstone {
            return Ok(());
        }

        let record = NodeRecord {
            tombstone: true,
            updated_at: now(),
            ..existing
        };
        let payload = serde_json::to_value(Record::Node(record.clone())).unwrap();
        self.storage.put_object(
            monitor,
            token,
            Some(node_id),
            self.storage.current_version(node_id),
            payload,
        )?;
        index.apply(node_id, Record::Node(record));
        self.publish_changed(monitor, token, node_id.0, caller_boundary, Vec::new());
        Ok(())
    }

    /// The node's current `VersionId`, if it exists — the handle a caller needs to hold onto now
    /// in order to read this exact state back later via [`Self::get_at_version`], since nothing
    /// else in this crate's public API surfaces one (see [`Self::generation`]'s doc comment on
    /// why *that* pass-through deliberately returns a coarser `u64` instead, for the one existing
    /// caller a real version identity would be more than it needs).
    pub fn current_version(&self, node_id: NodeId) -> Option<VersionId> {
        self.storage.current_version(node_id)
    }

    /// A historical read: `node_id` as it existed at `version`, rather than its current value —
    /// docs/09 §5.1's real, durable-reference framing for a recovery point, which
    /// `hyperion-recovery`'s own doc comment names as blocked on this not existing (this crate's
    /// live [`index`](crate::index) only ever holds the *current* value per node). Reads through
    /// directly to `hyperion-storage`'s own version chain — `StorageEngine::get_object`'s
    /// `version` parameter already supported this; nothing needed to change there, only a caller
    /// on this side that asks for it. `Err(GraphError::NotFound)` covers both "no such version"
    /// and "that version belongs to an edge, not a node" — a caller with the wrong id shape gets
    /// the same "not found" this crate already returns for [`Self::get`], not a different error
    /// shape to special-case. Owner-checked (2026-07-16) the same way [`Self::get`] now is: a
    /// node's `owner` never changes across its lifetime (see [`Self::put_node`]'s own doc
    /// comment), so checking it against the crate's own live `index` is exactly as correct for a
    /// historical version as for the current one.
    pub fn get_at_version(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        node_id: NodeId,
        version: VersionId,
    ) -> Result<NodeRecord, GraphError> {
        self.require(monitor, token, RightsMask::READ)?;
        let caller_boundary = token.origin().0;
        {
            let index = self.index.lock().unwrap();
            match index.nodes.get(&node_id) {
                Some(n) if n.owner == caller_boundary => {}
                _ => return Err(GraphError::NotFound),
            }
        }
        let payload = match self
            .storage
            .get_object(monitor, token, node_id, Some(version))
        {
            Ok(payload) => payload,
            Err(hyperion_storage::StorageError::NotFound) => return Err(GraphError::NotFound),
            Err(e) => return Err(e.into()),
        };
        match serde_json::from_value::<Record>(payload).map_err(|_| GraphError::NotFound)? {
            Record::Node(record) => Ok(record),
            Record::Edge(_) => Err(GraphError::NotFound),
        }
    }

    /// `graph.query` — docs/09 §6/§7: type filter ∩ vector similarity ∩
    /// temporal window ∩ edge constraint, ranked by similarity, over
    /// exactly the caller's own Trust Boundary's objects — docs/09 §8's
    /// "capability-checked at every hop," per-object, not merely the
    /// coarse per-call rights check [`Self::require`] alone gives. A
    /// candidate owned by a different boundary is excluded entirely,
    /// never merely down-ranked, mirroring `hyperion-context::engine`'s
    /// own downstream filter of the same shape (now redundant there, but
    /// left in place — defense in depth, not dead code).
    pub fn query(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        query: &GraphQuery,
    ) -> Result<Vec<QueryHit>, GraphError> {
        self.require(monitor, token, RightsMask::READ)?;
        let index = self.index.lock().unwrap();
        let caller_boundary = token.origin().0;

        let mut hits: Vec<QueryHit> = index
            .nodes
            .iter()
            .filter(|(_, n)| !n.tombstone && n.owner == caller_boundary)
            .filter(|(_, n)| {
                query
                    .type_filter
                    .as_ref()
                    .is_none_or(|types| types.contains(&n.object_type))
            })
            .filter(|(_, n)| {
                query
                    .time_range
                    .is_none_or(|(lo, hi)| n.created_at >= lo && n.created_at <= hi)
            })
            .filter(|(id, _)| {
                query
                    .edge_constraint
                    .as_ref()
                    .is_none_or(|c| Self::satisfies_edge_constraint(&index, **id, c))
            })
            .map(|(id, n)| {
                let score = match (&query.embedding_query, &n.embedding) {
                    (Some(q), Some(e)) => cosine_similarity(q, e),
                    (Some(_), None) => 0.0,
                    (None, _) => 1.0,
                };
                QueryHit {
                    node_id: *id,
                    node: n.clone(),
                    score,
                }
            })
            .collect();

        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if query.limit > 0 {
            hits.truncate(query.limit);
        }
        Ok(hits)
    }

    /// `graph.dump` -- every live node and edge the caller's own Trust Boundary can see, as one
    /// [`GraphSnapshot`]. Unlike [`Self::query`] (nodes only, ranked and optionally truncated) or
    /// [`Self::traverse`] (a bounded-hop walk from one anchor), this is the whole visible graph in
    /// one call -- built for a caller that wants to inspect or diff the graph's structure itself
    /// (e.g. `hyperion-console`'s own `/graph` meta-command, run before and after a scenario to
    /// see what changed), not to answer a specific question about it. Real, current scale (docs/41
    /// Phase 2/3's own scenario runs: dozens of nodes/edges per session, not thousands) makes a
    /// full, unbounded scan the right call here -- no `limit`, unlike `query`.
    pub fn dump(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
    ) -> Result<GraphSnapshot, GraphError> {
        self.require(monitor, token, RightsMask::READ)?;
        let index = self.index.lock().unwrap();
        let caller_boundary = token.origin().0;

        let mut nodes: Vec<(NodeId, NodeRecord)> = index
            .nodes
            .iter()
            .filter(|(_, n)| !n.tombstone && n.owner == caller_boundary)
            .map(|(id, n)| (*id, n.clone()))
            .collect();
        nodes.sort_by_key(|(id, _)| *id);

        let mut edges: Vec<(EdgeId, EdgeRecord)> = index
            .edges
            .iter()
            .filter(|(_, e)| !e.tombstone && e.owner == caller_boundary)
            .map(|(id, e)| (*id, e.clone()))
            .collect();
        edges.sort_by_key(|(id, _)| *id);

        Ok(GraphSnapshot { nodes, edges })
    }

    fn satisfies_edge_constraint(
        index: &GraphIndex,
        node: NodeId,
        constraint: &EdgeConstraint,
    ) -> bool {
        let forward_hit = index
            .forward
            .get(&node)
            .into_iter()
            .flatten()
            .filter_map(|id| index.active_edge(*id))
            .any(|e| e.predicate == constraint.predicate && e.target == constraint.node);
        let backward_hit = index
            .backward
            .get(&node)
            .into_iter()
            .flatten()
            .filter_map(|id| index.active_edge(*id))
            .any(|e| e.predicate == constraint.predicate && e.subject == constraint.node);
        forward_hit || backward_hit
    }

    /// `graph.traverse` — docs/09 §6, implementing docs/29 §Algorithms'
    /// bidirectional-union recursive query: at every hop, edges are followed
    /// in both directions from the current frontier so "everything related
    /// to X" finds objects that point *at* the anchor as well as objects the
    /// anchor points at. docs/09 §8's "capability-checked at every hop, not
    /// merely at the query boundary" is now real, per-object: the traversal
    /// never expands *into* a node outside the caller's own Trust Boundary
    /// (excluded entirely — its edge is never marked visited either — not
    /// merely omitted from the final result after being walked), and `start`
    /// itself is treated as not-found if the caller doesn't own it, the same
    /// "never reveal existence of what you can't see" shape [`Self::get`]
    /// already gives a single node.
    pub fn traverse(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        start: NodeId,
        edge_types: Option<&[String]>,
        max_hops: usize,
    ) -> Result<Subgraph, GraphError> {
        self.require(monitor, token, RightsMask::READ)?;
        let index = self.index.lock().unwrap();
        let caller_boundary = token.origin().0;
        let visible = |id: &NodeId| {
            index
                .nodes
                .get(id)
                .is_some_and(|n| !n.tombstone && n.owner == caller_boundary)
        };
        if !visible(&start) {
            return Err(GraphError::NotFound);
        }

        let mut depths = std::collections::HashMap::new();
        let mut visited_edges = HashSet::new();
        depths.insert(start, 0usize);
        let mut frontier = vec![start];

        for hop in 0..max_hops {
            if frontier.is_empty() {
                break;
            }
            let mut next_frontier = Vec::new();
            for node in &frontier {
                for &eid in index.forward.get(node).into_iter().flatten() {
                    let Some(edge) = index.active_edge(eid) else {
                        continue;
                    };
                    if edge_types.is_some_and(|types| !types.contains(&edge.predicate)) {
                        continue;
                    }
                    if !visible(&edge.target) {
                        continue;
                    }
                    visited_edges.insert(eid);
                    if let std::collections::hash_map::Entry::Vacant(slot) =
                        depths.entry(edge.target)
                    {
                        slot.insert(hop + 1);
                        next_frontier.push(edge.target);
                    }
                }
                for &eid in index.backward.get(node).into_iter().flatten() {
                    let Some(edge) = index.active_edge(eid) else {
                        continue;
                    };
                    if edge_types.is_some_and(|types| !types.contains(&edge.predicate)) {
                        continue;
                    }
                    if !visible(&edge.subject) {
                        continue;
                    }
                    visited_edges.insert(eid);
                    if let std::collections::hash_map::Entry::Vacant(slot) =
                        depths.entry(edge.subject)
                    {
                        slot.insert(hop + 1);
                        next_frontier.push(edge.subject);
                    }
                }
            }
            frontier = next_frontier;
        }

        let nodes = depths
            .into_iter()
            .map(|(id, depth)| (id, index.nodes[&id].clone(), depth))
            .collect();
        let edges = visited_edges
            .into_iter()
            .map(|id| (id, index.edges[&id].clone()))
            .collect();
        Ok(Subgraph { nodes, edges })
    }

    /// The node's current logical generation — its `updated_at` timestamp,
    /// which advances on every write. A stand-in for a real per-object
    /// version counter distinct from wall-clock time; see this crate's doc
    /// comment. Exists specifically so [07 — Context
    /// Propagation](../07-context-propagation.md)'s staleness check
    /// (`per_entry_generation`) has something to compare against without
    /// this crate exposing its internal `hyperion_storage::VersionId`.
    pub fn generation(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        node_id: NodeId,
    ) -> Result<u64, GraphError> {
        self.get(monitor, token, node_id).map(|n| n.updated_at)
    }

    /// `graph.explain` — docs/09 §6, feeding
    /// [18 — Explainability & Trust](../18-explainability-and-trust.md). Owner-checked
    /// (2026-07-16) the same way [`Self::get`] now is: this returns `owner`/`provenance`
    /// verbatim, so leaving it unchecked would leak exactly the cross-boundary metadata docs/09
    /// §8's "capability-checked at every hop" exists to prevent.
    pub fn explain(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        target: ExplainRef,
    ) -> Result<ProvenanceChain, GraphError> {
        self.require(monitor, token, RightsMask::READ)?;
        let caller_boundary = token.origin().0;
        let index = self.index.lock().unwrap();
        match target {
            ExplainRef::Node(id) => {
                let node = index
                    .nodes
                    .get(&id)
                    .filter(|n| n.owner == caller_boundary)
                    .ok_or(GraphError::NotFound)?;
                let mut incident_edges: Vec<EdgeId> = index
                    .forward
                    .get(&id)
                    .into_iter()
                    .flatten()
                    .chain(index.backward.get(&id).into_iter().flatten())
                    .copied()
                    .collect();
                incident_edges.sort_by_key(|e| e.0);
                incident_edges.dedup();
                Ok(ProvenanceChain::Node {
                    node_id: id,
                    object_type: node.object_type.clone(),
                    owner: node.owner,
                    created_at: node.created_at,
                    updated_at: node.updated_at,
                    incident_edges,
                })
            }
            ExplainRef::Edge(id) => {
                let edge = index
                    .edges
                    .get(&id)
                    .filter(|e| e.owner == caller_boundary)
                    .ok_or(GraphError::NotFound)?;
                Ok(ProvenanceChain::Edge {
                    edge_id: id,
                    subject: edge.subject,
                    predicate: edge.predicate.clone(),
                    target: edge.target,
                    origin: edge.origin,
                    provenance: edge.provenance.clone(),
                    confidence: edge.confidence,
                    created_at: edge.created_at,
                    tombstone: edge.tombstone,
                })
            }
        }
    }
}
