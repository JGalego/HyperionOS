use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use hyperion_capability::{CapabilityMonitor, CapabilityToken};
use hyperion_knowledge_graph::{EdgeOrigin, GraphError, GraphQuery, KnowledgeGraph, NodeId};
use hyperion_scheduler::{
    IntentId, ResourceDimension, ResourceLedger, ResourceVector, SchedClass, Scheduler,
    TaskDescriptor, TaskId,
};

use crate::decay::{decay_score, is_promotable, THETA_ARCHIVE, THETA_PROMOTE};
use crate::types::{MemoryRecord, MemoryTier};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs()
}

const ALL_TIERS: [MemoryTier; 4] = [
    MemoryTier::Episodic,
    MemoryTier::Semantic,
    MemoryTier::Procedural,
    MemoryTier::LongTerm,
];

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("knowledge graph error: {0}")]
    Graph(#[from] GraphError),
    #[error("no such memory record")]
    NotFound,
    #[error("scheduler error: {0}")]
    Scheduler(#[from] hyperion_scheduler::SchedError),
}

/// docs/08 §6's `memory.query` filter.
#[derive(Debug, Clone, Default)]
pub struct MemoryFilter {
    pub tier: Option<MemoryTier>,
    pub pinned_only: bool,
    pub include_dormant: bool,
    pub include_erased: bool,
    pub time_range: Option<(u64, u64)>,
}

#[derive(Debug, Clone)]
pub struct ErasureReceipt {
    pub id: NodeId,
    /// Dependent facts also erased — docs/08 §6's `cascade: bool = true`.
    pub cascaded: Vec<NodeId>,
}

#[derive(Debug, Clone)]
pub struct ExtractionReceipt {
    pub promoted: Vec<NodeId>,
}

/// docs/08 — Memory Engine, as a typed view over a real
/// [`hyperion_knowledge_graph::KnowledgeGraph`]. See this crate's doc
/// comment for what's deferred.
pub struct MemoryEngine {
    graph: Arc<KnowledgeGraph>,
    /// Backs [`Self::run_co_occurrence_pass`]'s real Scheduler admission —
    /// see this crate's doc comment.
    scheduler: Mutex<Scheduler>,
    next_task_id: AtomicU64,
}

impl MemoryEngine {
    pub fn new(graph: Arc<KnowledgeGraph>) -> Self {
        let mut scheduler = Scheduler::new();
        scheduler.register_resource_provider(ResourceLedger::new(ResourceDimension::Cpu, 100, 0));
        MemoryEngine {
            graph,
            scheduler: Mutex::new(scheduler),
            next_task_id: AtomicU64::new(1),
        }
    }

    fn to_record(
        node_id: NodeId,
        node: hyperion_knowledge_graph::NodeRecord,
    ) -> Option<MemoryRecord> {
        let mut record: MemoryRecord = serde_json::from_value(node.metadata).ok()?;
        record.id = node_id;
        Some(record)
    }

    /// `memory.remember` — docs/08 §6. `pin=true` bypasses decay entirely
    /// (§5.2: "if r.pinned: score := 1.0").
    #[allow(clippy::too_many_arguments)]
    pub fn remember(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        tier: MemoryTier,
        content: serde_json::Value,
        embedding: Option<Vec<f32>>,
        importance: f32,
        pinned: bool,
        provenance: Vec<NodeId>,
    ) -> Result<NodeId, MemoryError> {
        let ts = now();
        let record = MemoryRecord {
            id: hyperion_storage::ObjectId(0), // placeholder; never serialized, see MemoryRecord::id

            tier,
            content,
            embedding: embedding.clone(),
            created_at: ts,
            last_accessed_at: ts,
            access_count: 0,
            importance,
            decay_score: if pinned { 1.0 } else { importance },
            pinned,
            provenance,
            erased: false,
            dormant: false,
        };
        let metadata = serde_json::to_value(&record).expect("MemoryRecord always serializes");
        let id = self.graph.put_node(
            monitor,
            token,
            None,
            tier.as_object_type(),
            embedding,
            metadata,
        )?;
        Ok(id)
    }

    /// `remember_explicit` — docs/08 §5.2/§7: bypasses decay entirely for
    /// an explicit user "remember that..."; mirrors to Long-Term
    /// immediately rather than waiting for consolidation.
    pub fn remember_explicit(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        fact: serde_json::Value,
        embedding: Option<Vec<f32>>,
    ) -> Result<(NodeId, NodeId), MemoryError> {
        let semantic_id = self.remember(
            monitor,
            token,
            MemoryTier::Semantic,
            fact.clone(),
            embedding.clone(),
            1.0,
            true,
            Vec::new(),
        )?;
        let long_term_id = self.remember(
            monitor,
            token,
            MemoryTier::LongTerm,
            serde_json::json!({"consolidated_from": semantic_id.0, "content": fact}),
            embedding,
            1.0,
            true,
            vec![semantic_id],
        )?;
        Ok((semantic_id, long_term_id))
    }

    /// `memory.query` — docs/08 §6.
    pub fn query(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        filter: &MemoryFilter,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        let type_filter = match filter.tier {
            Some(tier) => vec![tier.as_object_type().to_string()],
            None => ALL_TIERS
                .iter()
                .map(|t| t.as_object_type().to_string())
                .collect(),
        };
        let hits = self.graph.query(
            monitor,
            token,
            &GraphQuery {
                type_filter: Some(type_filter),
                time_range: filter.time_range,
                limit: 0,
                ..Default::default()
            },
        )?;

        Ok(hits
            .into_iter()
            .filter_map(|h| Self::to_record(h.node_id, h.node))
            .filter(|r| !r.erased || filter.include_erased)
            .filter(|r| !r.dormant || filter.include_dormant)
            .filter(|r| !filter.pinned_only || r.pinned)
            .collect())
    }

    /// `memory.recall` — docs/08 §6: ranked retrieval, deprioritizing (by
    /// omission) Dormant records from the default result, per §5.3.
    pub fn recall(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        embedding: Vec<f32>,
        k: usize,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        let type_filter = ALL_TIERS
            .iter()
            .map(|t| t.as_object_type().to_string())
            .collect();
        let hits = self.graph.query(
            monitor,
            token,
            &GraphQuery {
                type_filter: Some(type_filter),
                embedding_query: Some(embedding),
                limit: 0,
                ..Default::default()
            },
        )?;

        let mut records: Vec<MemoryRecord> = hits
            .into_iter()
            .filter_map(|h| Self::to_record(h.node_id, h.node))
            .filter(|r| !r.erased && !r.dormant)
            .collect();
        records.truncate(k);

        // docs/08 §4: `last_accessed_at`/`access_count` drive §5.2's decay
        // score — a retrieval that never updates them would let a record
        // decay as if it were never used, which is exactly backwards. This
        // is `recall`'s footprint specifically; `query` (a browse/inspect
        // operation for the transparency API, §6) has none.
        let now_ts = now();
        for record in &mut records {
            record.last_accessed_at = now_ts;
            record.access_count += 1;
            self.rewrite(monitor, token, record)?;
        }
        Ok(records)
    }

    /// `memory.explain` — docs/08 §6.
    pub fn explain(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: NodeId,
    ) -> Result<hyperion_knowledge_graph::ProvenanceChain, MemoryError> {
        Ok(self.graph.explain(
            monitor,
            token,
            hyperion_knowledge_graph::ExplainRef::Node(id),
        )?)
    }

    /// `memory.edit` — docs/08 §6: a user correction merges into `content`
    /// as a JSON-object shallow merge, versioned via the underlying
    /// Knowledge Graph write (never overwritten in place at the storage
    /// layer — docs/02 §4 invariant #2).
    pub fn edit(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: NodeId,
        patch: serde_json::Value,
    ) -> Result<MemoryRecord, MemoryError> {
        let node = self.graph.get(monitor, token, id)?;
        let mut record: MemoryRecord =
            serde_json::from_value(node.metadata).map_err(|_| MemoryError::NotFound)?;
        record.id = id;
        merge_json(&mut record.content, &patch);
        self.rewrite(monitor, token, &record)?;
        Ok(record)
    }

    fn rewrite(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        record: &MemoryRecord,
    ) -> Result<(), MemoryError> {
        let metadata = serde_json::to_value(record).expect("MemoryRecord always serializes");
        self.graph.put_node(
            monitor,
            token,
            Some(record.id),
            record.tier.as_object_type(),
            record.embedding.clone(),
            metadata,
        )?;
        Ok(())
    }

    pub fn pin(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: NodeId,
    ) -> Result<(), MemoryError> {
        self.set_pinned(monitor, token, id, true)
    }

    pub fn unpin(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: NodeId,
    ) -> Result<(), MemoryError> {
        self.set_pinned(monitor, token, id, false)
    }

    fn set_pinned(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: NodeId,
        pinned: bool,
    ) -> Result<(), MemoryError> {
        let node = self.graph.get(monitor, token, id)?;
        let mut record: MemoryRecord =
            serde_json::from_value(node.metadata).map_err(|_| MemoryError::NotFound)?;
        record.id = id;
        record.pinned = pinned;
        self.rewrite(monitor, token, &record)
    }

    /// `memory.erase` — docs/08 §6: SoftDelete only (see this crate's doc
    /// comment). `cascade` also erases any record whose `provenance`
    /// names `id` (a fact extracted *from* the erased record).
    pub fn erase(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: NodeId,
        cascade: bool,
    ) -> Result<ErasureReceipt, MemoryError> {
        let node = self.graph.get(monitor, token, id)?;
        let mut record: MemoryRecord =
            serde_json::from_value(node.metadata).map_err(|_| MemoryError::NotFound)?;
        record.id = id;
        record.erased = true;
        self.rewrite(monitor, token, &record)?;

        let mut cascaded = Vec::new();
        if cascade {
            let dependents = self.query(monitor, token, &MemoryFilter::default())?;
            for dependent in dependents {
                if dependent.provenance.contains(&id) && !dependent.erased {
                    self.erase(monitor, token, dependent.id, true)?;
                    cascaded.push(dependent.id);
                }
            }
        }
        Ok(ErasureReceipt { id, cascaded })
    }

    /// `memory.export` — docs/08 §6: a full portable export.
    pub fn export(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        filter: &MemoryFilter,
    ) -> Result<serde_json::Value, MemoryError> {
        let records = self.query(monitor, token, filter)?;
        Ok(serde_json::to_value(records).expect("records always serialize"))
    }

    /// docs/08 §7 `consolidation_cycle`'s decay half: recompute every
    /// non-pinned record's `decay_score`, promoting to Long-Term at or
    /// above [`THETA_PROMOTE`] and marking Dormant below [`THETA_ARCHIVE`].
    /// `THETA_PURGE` is never checked here — purge is user/policy-initiated
    /// only (§5.3).
    pub fn run_decay_pass(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
    ) -> Result<ExtractionReceipt, MemoryError> {
        let now_ts = now();
        let records = self.query(
            monitor,
            token,
            &MemoryFilter {
                include_dormant: true,
                ..Default::default()
            },
        )?;

        let mut promoted = Vec::new();
        for mut record in records {
            if record.pinned {
                continue;
            }
            let score = decay_score(&record, now_ts);
            record.decay_score = score;
            record.dormant = score < THETA_ARCHIVE;
            if score >= THETA_PROMOTE && is_promotable(record.tier) {
                let long_term_id = self.remember(
                    monitor,
                    token,
                    MemoryTier::LongTerm,
                    serde_json::json!({"consolidated_from": record.id.0, "content": record.content}),
                    record.embedding.clone(),
                    record.importance,
                    false,
                    vec![record.id],
                )?;
                promoted.push(long_term_id);
            }
            self.rewrite(monitor, token, &record)?;
        }
        Ok(ExtractionReceipt { promoted })
    }

    /// docs/08 §5.4/§7's extraction half of `consolidation_cycle`: groups
    /// non-erased, not-yet-consolidated Episodic records sharing the same
    /// caller-supplied `entity_key`/`fact` pair (see this crate's doc
    /// comment on the embedding-clustering deferral) and promotes any group
    /// with `count >= min_occurrences` to a Semantic record with
    /// `confidence = 1 - 0.5^count` — the frequency gate that prevents a
    /// one-off event from being mislearned as a standing preference.
    pub fn run_extraction_pass(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        min_occurrences: usize,
    ) -> Result<ExtractionReceipt, MemoryError> {
        let episodes = self.query(
            monitor,
            token,
            &MemoryFilter {
                tier: Some(MemoryTier::Episodic),
                include_dormant: true,
                ..Default::default()
            },
        )?;

        let mut groups: std::collections::HashMap<(String, String), Vec<MemoryRecord>> =
            std::collections::HashMap::new();
        for episode in episodes {
            let already_consolidated = episode
                .content
                .get("consolidated")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if already_consolidated {
                continue;
            }
            let entity_key = episode.content.get("entity_key").and_then(|v| v.as_str());
            let fact = episode.content.get("fact").and_then(|v| v.as_str());
            if let (Some(entity_key), Some(fact)) = (entity_key, fact) {
                groups
                    .entry((entity_key.to_string(), fact.to_string()))
                    .or_default()
                    .push(episode);
            }
        }

        let mut promoted = Vec::new();
        for ((entity_key, fact), episodes) in groups {
            if episodes.len() < min_occurrences {
                continue;
            }
            let count = episodes.len();
            let confidence = 1.0 - 0.5_f32.powi(count as i32);
            let provenance: Vec<NodeId> = episodes.iter().map(|e| e.id).collect();

            let semantic_id = self.remember(
                monitor,
                token,
                MemoryTier::Semantic,
                serde_json::json!({"entity_key": entity_key, "fact": fact, "confidence": confidence}),
                None,
                confidence,
                false,
                provenance,
            )?;
            promoted.push(semantic_id);

            for mut episode in episodes {
                episode.content["consolidated"] = serde_json::json!(true);
                self.rewrite(monitor, token, &episode)?;
            }
        }
        Ok(ExtractionReceipt { promoted })
    }

    /// docs/998-roadmap.md's Backlog "Protect the Human" item: "no signal exists for 'you've
    /// delegated this kind of task N times this month, want to do the next one yourself?'" —
    /// `hyperion-memory`'s procedural tier was already named as the likely home for this ("it
    /// already tracks repeated task patterns"). Counts non-erased Procedural records whose
    /// caller-supplied `content.entity_key` matches `entity_key` and whose `created_at` falls at
    /// or after `since_ts` — the same explicit-`entity_key` grouping convention
    /// [`Self::run_extraction_pass`] already established, reused here instead of inventing a
    /// second one. Deliberately returns a plain count, not a decision: whether/how a caller acts
    /// on it (e.g. surfacing "want to do the next one yourself?") is not this crate's call — see
    /// `hyperion-api-gateway::check_skill_delegation_signal` for the real, explainable bridge that
    /// consumes it.
    pub fn count_procedural_delegations(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        entity_key: &str,
        since_ts: u64,
    ) -> Result<crate::types::DelegationCount, MemoryError> {
        let records = self.query(
            monitor,
            token,
            &MemoryFilter {
                tier: Some(MemoryTier::Procedural),
                time_range: Some((since_ts, u64::MAX)),
                ..Default::default()
            },
        )?;
        let count = records
            .iter()
            .filter(|r| r.content.get("entity_key").and_then(|v| v.as_str()) == Some(entity_key))
            .count();
        Ok(crate::types::DelegationCount {
            entity_key: entity_key.to_string(),
            count,
            window_start: since_ts,
        })
    }

    /// docs/09 §5.2's inferred-edge background job, the `co-occurs-with`
    /// half: sourced from this crate's own real `MemoryRecord.provenance`
    /// (every Knowledge Graph object a memory record actually names),
    /// every pair of objects named by the same record gets a real
    /// `co-occurs-with` edge — not the `semantically-similar-to` half of
    /// the same deferred item, which still needs real embeddings this
    /// workspace doesn't have. Submitted as a real `hyperion-scheduler`
    /// `BatchDistributable` task first (closing the other half of "needs
    /// a scheduler-driven background job"), matching the same real-
    /// admission pattern `hyperion-agent-runtime`'s own quota gate uses.
    /// Idempotent per call: repeated co-occurrence sets the edge's weight
    /// to `1.0` again rather than accumulating it — real weight
    /// accumulation/decay for these inferred edges is a further,
    /// separate refinement this pass doesn't attempt.
    pub fn run_co_occurrence_pass(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
    ) -> Result<usize, MemoryError> {
        let task_id = TaskId(self.next_task_id.fetch_add(1, Ordering::Relaxed));
        let ticket = {
            let mut scheduler = self.scheduler.lock().unwrap();
            let ticket = scheduler.submit_task(
                monitor,
                TaskDescriptor {
                    id: task_id,
                    owner_intent: IntentId(0),
                    owner_agent: None,
                    class: SchedClass::BatchDistributable,
                    deadline: None,
                    priority_weight: 0.5,
                    request: ResourceVector {
                        cpu_shares: 1,
                        ..Default::default()
                    },
                    cap_token: token.clone(),
                },
            )?;
            scheduler.schedule_epoch();
            ticket
        };

        let records = self.query(
            monitor,
            token,
            &MemoryFilter {
                include_dormant: true,
                ..Default::default()
            },
        )?;

        let mut edges_touched = 0;
        for record in &records {
            for i in 0..record.provenance.len() {
                for j in (i + 1)..record.provenance.len() {
                    self.graph.link(
                        monitor,
                        token,
                        record.provenance[i],
                        "co-occurs-with",
                        record.provenance[j],
                        1.0,
                        EdgeOrigin::Inferred,
                        None,
                        "memory_co_occurrence",
                        None,
                    )?;
                    edges_touched += 1;
                }
            }
        }

        let _ = self.scheduler.lock().unwrap().complete(ticket);
        Ok(edges_touched)
    }
}

/// A shallow JSON-object merge for [`MemoryEngine::edit`] — a patch key
/// overwrites the matching content key; nested objects are not deep-merged
/// (docs/08 doesn't specify patch semantics beyond "user corrects a fact
/// directly," and a shallow merge is the simplest faithful reading).
fn merge_json(base: &mut serde_json::Value, patch: &serde_json::Value) {
    if let (serde_json::Value::Object(base_map), serde_json::Value::Object(patch_map)) =
        (base, patch)
    {
        for (k, v) in patch_map {
            base_map.insert(k.clone(), v.clone());
        }
    }
}
