use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_context::{ContextEngine, EntityResolution};
use hyperion_events::{
    BackpressurePolicy, DeliveryClass, EventBus, EventPayload, Subscription, TopicKind,
    TopicPattern,
};
use hyperion_knowledge_graph::{EdgeOrigin, GraphError, GraphQuery, KnowledgeGraph, NodeId};

use crate::path::PathMappingCache;
use crate::types::{DirEntry, QueryCacheKey, QuerySpec, VirtualFolder};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs()
}

#[derive(Debug, thiserror::Error)]
pub enum FsError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("knowledge graph error: {0}")]
    Graph(#[from] GraphError),
    #[error("context engine error: {0}")]
    Context(#[from] hyperion_context::ContextError),
    #[error("no such virtual folder")]
    FolderNotFound,
    #[error("no such path")]
    PathNotFound,
    #[error("query has no anchor to resolve from")]
    NoAnchor,
}

/// docs/10 §Recovery Mechanisms: "ambiguous anchor resolution falls back to
/// presenting the candidates for disambiguation rather than silently
/// guessing" — the outcome of [`SemanticFilesystem::resolve_query_from_mention`].
#[derive(Debug, Clone)]
pub enum AnchorResolution {
    Spec(QuerySpec),
    Ambiguous(Vec<NodeId>),
    NotFound,
}

fn sanitize_path_segment(s: &str) -> String {
    s.replace('/', "-")
}

/// docs/10 — Semantic Filesystem, over a real
/// [`hyperion_knowledge_graph::KnowledgeGraph`]. See this crate's doc
/// comment for what's deferred.
pub struct SemanticFilesystem {
    graph: Arc<KnowledgeGraph>,
    context: Arc<ContextEngine>,
    folders: Mutex<HashMap<u64, VirtualFolder>>,
    /// docs/10 §Performance Analysis's own "cached with a TTL and invalidated incrementally"
    /// claim, made real for the first time: reuses an existing, still-fresh `VirtualFolder` for a
    /// repeat query of the same structural shape (see [`QueryCacheKey`]) instead of always
    /// re-materializing (this crate's previous, real behavior — every `query()` call minted a
    /// brand new folder unconditionally).
    folder_cache: Mutex<HashMap<QueryCacheKey, u64>>,
    path_cache: Mutex<PathMappingCache>,
    next_id: AtomicU64,
    /// docs/10 §Performance Analysis's own named gap: "the Query Resolver subscribes to relevant
    /// object/edge changes via the Event System and only re-materializes the specific folders
    /// whose inputs actually changed." Real, optional, once [`Self::with_events`] wires it — see
    /// its own doc comment for why this is one shared kind-wide subscription rather than one per
    /// folder.
    invalidation: Option<Subscription>,
}

impl SemanticFilesystem {
    pub fn new(graph: Arc<KnowledgeGraph>, context: Arc<ContextEngine>) -> Self {
        SemanticFilesystem {
            graph,
            context,
            folders: Mutex::new(HashMap::new()),
            folder_cache: Mutex::new(HashMap::new()),
            path_cache: Mutex::new(PathMappingCache::default()),
            next_id: AtomicU64::new(1),
            invalidation: None,
        }
    }

    /// Opts this filesystem into docs/31-event-system.md's real broadcast bus, closing this
    /// crate's own previously-named "no live re-materialization to race against yet" gap for
    /// real: once wired, a cached `VirtualFolder` is evicted the moment any Knowledge-Graph
    /// write touches one of its own member objects or its query anchor (see
    /// [`hyperion_knowledge_graph::KnowledgeGraph::with_events`], the publisher this subscribes
    /// to), so the *next* `query()` for that shape re-materializes fresh rather than serving
    /// stale membership until the cache's own TTL happens to expire.
    ///
    /// One shared `KindScoped(ObjectChanged)` subscription, not one `Exact` subscription per
    /// cached folder: a personal-scale graph can have many live folders at once, and this crate
    /// has no background thread to prune subscriptions for folders nobody queries again — a
    /// single subscription plus a local scan (see [`Self::drain_invalidations`]) is real,
    /// correct, and does not need active lifecycle management as folders come and go.
    /// `admin_token` must carry `GRANT`, the same elevated cross-subject visibility a docs/34
    /// audit sink already needs for the identical `KindScoped` reason.
    pub fn with_events(
        mut self,
        monitor: &CapabilityMonitor,
        admin_token: &CapabilityToken,
        events: Arc<EventBus>,
    ) -> Result<Self, FsError> {
        let sub = events
            .subscribe(
                monitor,
                admin_token,
                admin_token.origin(),
                TopicPattern::KindScoped(TopicKind::ObjectChanged),
                DeliveryClass::AtMostOnce,
                BackpressurePolicy::Buffer { capacity: 256 },
            )
            .map_err(|_| FsError::Unauthorized)?;
        self.invalidation = Some(sub);
        Ok(self)
    }

    /// Drains every pending `ObjectChanged` event and evicts any cached folder whose anchor or
    /// membership it touches. Called at the top of [`Self::query`] rather than from a background
    /// thread — this crate's own hosted-simulator convention (see e.g.
    /// `hyperion-explainability::ExplanationStore`) of doing housekeeping lazily on the next real
    /// call rather than running a daemon thread nothing else in this workspace does either.
    fn drain_invalidations(&self) {
        let Some(sub) = &self.invalidation else {
            return;
        };
        let mut changed_ids: Vec<NodeId> = Vec::new();
        while let Some(event) = sub.try_recv() {
            // A node's own change is its own subject; an edge's change is relevant to the two
            // *nodes* it connects, not to the edge's own id (a different, uninteresting id to
            // this crate's folders) -- `hyperion_knowledge_graph::KnowledgeGraph::link`/`unlink`
            // publish those two node ids as this event's own `ancestors` for exactly this reason.
            if let EventPayload::Inline(payload) = &event.payload {
                if let Some(id) = payload["object_id"].as_u64() {
                    changed_ids.push(hyperion_storage::ObjectId(id));
                }
            }
            changed_ids.extend(
                event
                    .ancestors
                    .iter()
                    .map(|&id| hyperion_storage::ObjectId(id)),
            );
        }
        if changed_ids.is_empty() {
            return;
        }
        let mut folders = self.folders.lock().unwrap();
        let mut cache = self.folder_cache.lock().unwrap();
        let stale: Vec<u64> = folders
            .iter()
            .filter(|(_, folder)| {
                folder
                    .query_spec
                    .anchor
                    .is_some_and(|anchor| changed_ids.contains(&anchor))
                    || folder
                        .member_object_ids
                        .iter()
                        .any(|member| changed_ids.contains(member))
            })
            .map(|(id, _)| *id)
            .collect();
        for id in &stale {
            folders.remove(id);
        }
        cache.retain(|_, folder_id| !stale.contains(folder_id));
    }

    fn require(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        rights: RightsMask,
    ) -> Result<(), FsError> {
        monitor
            .check_rights_ok_result(token, rights)
            .map_err(|_| FsError::Unauthorized)
    }

    /// "Universal search"'s front door — docs/10 §Algorithms step 1, via
    /// `hyperion-context`'s own entity resolution rather than a second
    /// grounding mechanism.
    pub fn resolve_query_from_mention(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        mention: &str,
        session_id: &str,
        hop_bound: usize,
        k: usize,
    ) -> Result<AnchorResolution, FsError> {
        match self
            .context
            .resolve_entity(monitor, token, mention, session_id)?
        {
            EntityResolution::Resolved { node_id, .. } => Ok(AnchorResolution::Spec(QuerySpec {
                anchor: Some(node_id),
                hop_bound,
                predicate_filter: None,
                type_filter: None,
                embedding: None,
                k,
                ttl_secs: 300,
            })),
            EntityResolution::Ambiguous(candidates) => Ok(AnchorResolution::Ambiguous(candidates)),
            EntityResolution::NotFound => Ok(AnchorResolution::NotFound),
        }
    }

    /// docs/10 §Algorithms' "Query resolution": bounded-hop relational
    /// traversal merged and deduplicated with a vector-similarity leg.
    pub fn query(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        spec: &QuerySpec,
    ) -> Result<VirtualFolder, FsError> {
        self.require(monitor, token, RightsMask::READ)?;
        let anchor = spec.anchor.ok_or(FsError::NoAnchor)?;

        self.drain_invalidations();
        // Caching is only ever consulted/populated once a real invalidation subscription is
        // wired (see `Self::with_events`) -- without one, nothing would ever evict a stale entry,
        // which would make `query()` silently return out-of-date membership forever instead of
        // this crate's own real, previously-unconditional "always re-materialize fresh" behavior.
        let cache_key = self
            .invalidation
            .as_ref()
            .and_then(|_| QueryCacheKey::from_spec(spec));
        if let Some(key) = &cache_key {
            let cached_id = self.folder_cache.lock().unwrap().get(key).copied();
            if let Some(folder_id) = cached_id {
                let folders = self.folders.lock().unwrap();
                if let Some(folder) = folders.get(&folder_id) {
                    if now() <= folder.materialized_at + folder.ttl_secs {
                        return Ok(folder.clone());
                    }
                }
                // Expired or already evicted by drain_invalidations -- fall through and
                // re-materialize fresh, replacing the stale cache entry below.
            }
        }

        let mut relational: Vec<(NodeId, usize)> = Vec::new();
        if spec.hop_bound > 0 {
            let subgraph = self.graph.traverse(
                monitor,
                token,
                anchor,
                spec.predicate_filter.as_deref(),
                spec.hop_bound,
            )?;
            relational = subgraph
                .nodes
                .into_iter()
                .map(|(id, _, depth)| (id, depth))
                .collect();
        } else {
            relational.push((anchor, 0));
        }
        relational.sort_by_key(|(_, depth)| *depth);

        let mut member_object_ids: Vec<NodeId> = relational.into_iter().map(|(id, _)| id).collect();

        if let Some(embedding) = spec.embedding.clone() {
            let hits = self.graph.query(
                monitor,
                token,
                &GraphQuery {
                    type_filter: spec.type_filter.clone(),
                    embedding_query: Some(embedding),
                    limit: spec.k,
                    ..Default::default()
                },
            )?;
            for hit in hits {
                if !member_object_ids.contains(&hit.node_id) {
                    member_object_ids.push(hit.node_id);
                }
            }
        }

        let folder_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let folder = VirtualFolder {
            folder_id,
            query_spec: spec.clone(),
            member_object_ids,
            materialized_at: now(),
            ttl_secs: spec.ttl_secs,
            // A VirtualFolder's member list never changes after creation
            // (see this crate's doc comment) — the folder's own id already
            // uniquely identifies that frozen state, so it doubles as the
            // snapshot token.
            snapshot_token: folder_id,
        };
        self.folders
            .lock()
            .unwrap()
            .insert(folder_id, folder.clone());
        if let Some(key) = cache_key {
            self.folder_cache.lock().unwrap().insert(key, folder_id);
        }
        Ok(folder)
    }

    /// docs/10 §Algorithms' "Path synthesis," re-checking each member's
    /// capability grant at materialization time rather than trusting the
    /// frozen membership list — docs/10 §Security Considerations.
    pub fn materialize(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        folder_id: u64,
    ) -> Result<Vec<DirEntry>, FsError> {
        self.require(monitor, token, RightsMask::READ)?;
        let folder = self
            .folders
            .lock()
            .unwrap()
            .get(&folder_id)
            .cloned()
            .ok_or(FsError::FolderNotFound)?;

        let mut candidates = Vec::with_capacity(folder.member_object_ids.len());
        for &id in &folder.member_object_ids {
            let node = self.graph.get(monitor, token, id)?;
            let title = node
                .metadata
                .get("title")
                .or_else(|| node.metadata.get("name"))
                .and_then(|v| v.as_str())
                .map(sanitize_path_segment)
                .unwrap_or_else(|| format!("object-{}", id.0));
            candidates.push((id, format!("{}/{title}", node.object_type)));
        }

        let paths = self
            .path_cache
            .lock()
            .unwrap()
            .synthesize_batch(&candidates);
        Ok(candidates
            .into_iter()
            .zip(paths)
            .map(|((id, _), path)| DirEntry {
                path,
                object_id: id,
            })
            .collect())
    }

    pub fn get_folder(&self, folder_id: u64) -> Option<VirtualFolder> {
        self.folders.lock().unwrap().get(&folder_id).cloned()
    }

    /// docs/10 §Algorithms' "Folder preservation": a real Semantic Object
    /// plus real explicit edges, never a filesystem-only directory entry.
    pub fn mkcollection(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        name: &str,
        parent: Option<NodeId>,
    ) -> Result<NodeId, FsError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let id = self.graph.put_node(
            monitor,
            token,
            None,
            "collection",
            None,
            serde_json::json!({"name": name}),
        )?;
        if let Some(parent_id) = parent {
            self.graph.link(
                monitor,
                token,
                id,
                "member_of",
                parent_id,
                1.0,
                EdgeOrigin::Explicit,
                None,
                "user_explicit",
                None,
            )?;
        }
        self.path_cache
            .lock()
            .unwrap()
            .pin(name.to_string(), id, true);
        Ok(id)
    }

    pub fn add_to_collection(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        object_id: NodeId,
        collection_id: NodeId,
    ) -> Result<(), FsError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        self.graph.link(
            monitor,
            token,
            object_id,
            "member_of",
            collection_id,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "user_explicit",
            None,
        )?;
        Ok(())
    }

    pub fn resolve_path(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        path: &str,
    ) -> Result<NodeId, FsError> {
        self.require(monitor, token, RightsMask::READ)?;
        self.path_cache
            .lock()
            .unwrap()
            .resolve(path)
            .ok_or(FsError::PathNotFound)
    }

    /// docs/10 §Algorithms' "Write-back": a write into a real user-created
    /// Collection fabricates an explicit `member_of` edge; a write into a
    /// virtual, query-materialized folder pins the path without inventing
    /// a false one — Design Invariant 1, no silent authority.
    pub fn write_back(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        path: &str,
        metadata: serde_json::Value,
    ) -> Result<NodeId, FsError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let existing = self.path_cache.lock().unwrap().resolve(path);
        let object_type = path
            .split('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("document");
        let id = self
            .graph
            .put_node(monitor, token, existing, object_type, None, metadata)?;

        if let Some((parent_path, _)) = path.rsplit_once('/') {
            let parent_lookup = self.path_cache.lock().unwrap().resolve(parent_path);
            if let Some(parent_id) = parent_lookup {
                let is_collection = self
                    .path_cache
                    .lock()
                    .unwrap()
                    .entry(parent_id)
                    .map(|e| e.is_collection)
                    .unwrap_or(false);
                if is_collection {
                    self.graph.link(
                        monitor,
                        token,
                        id,
                        "member_of",
                        parent_id,
                        1.0,
                        EdgeOrigin::Explicit,
                        None,
                        "user_explicit",
                        None,
                    )?;
                }
            }
        }
        self.path_cache
            .lock()
            .unwrap()
            .pin(path.to_string(), id, false);
        Ok(id)
    }
}
