use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use hyperion_ai_runtime::{CapabilityContract, InferenceRequest, LocalAiRuntime, ModelClass};
use hyperion_capability::{CapabilityMonitor, CapabilityToken};
use hyperion_knowledge_graph::{
    GraphError, GraphQuery, KnowledgeGraph, NodeId, NodeOrigin, NodeRecord, ProvenanceChain,
};
use hyperion_storage::ObjectId;

use crate::types::{
    Budget, ContextBundle, ContextEntry, ExpertiseEstimate, ExpertiseLevel, ExpertiseSignal,
    InclusionMode, Scope,
};
use crate::working_set::WorkingSet;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs()
}

/// docs/17 §5's real Provenance Trust Score, for docs/17 T4's own "Context retrieval weights
/// candidate objects by Provenance Trust Score" -- a narrowed, local copy of `hyperion-security::
/// kg_trust_score`'s *identical* formula, not a dependency on that crate: `hyperion-security`
/// already transitively depends on this crate (`hyperion-security -> hyperion-recovery ->
/// hyperion-agent-runtime -> hyperion-netstack -> hyperion-context`, confirmed by a real Cargo
/// cycle error when a direct dependency was tried), so the reverse direction is a hard cycle --
/// the same "define a local, narrowed copy in the crate that needs it" pattern this workspace
/// already uses for `hyperion-security::SensitivityHint`/`hyperion-explainability`'s
/// `RecoveryPointId`/`SensitivityClass`. Kept in sync by hand with `hyperion-security::
/// kg_trust_score`'s own doc comment, which carries the full rationale for every constant and
/// term here (origin tiers, corroboration saturation, age-based maturity, and the `[0.5, 1.0]`
/// demotion-not-exclusion floor) rather than repeating it in two places.
const CORROBORATION_SATURATION: f32 = 5.0;
const CORROBORATION_MAX_BONUS: f32 = 0.3;
const AGE_MATURITY_SECS: f32 = 24.0 * 3600.0;
const AGE_MAX_BONUS: f32 = 0.15;
const MIN_TRUST_SCORE: f32 = 0.5;

fn origin_base_score(origin: NodeOrigin) -> f32 {
    match origin {
        NodeOrigin::UserAuthored => 1.0,
        NodeOrigin::AgentGenerated => 0.85,
        NodeOrigin::SyncedRemote => 0.65,
        NodeOrigin::IngestedExternal => 0.4,
    }
}

fn kg_trust_score(node: &NodeRecord, now: u64) -> f32 {
    let corroboration_bonus = (node.corroboration_count as f32 / CORROBORATION_SATURATION).min(1.0)
        * CORROBORATION_MAX_BONUS;
    let age_secs = now.saturating_sub(node.created_at) as f32;
    let age_bonus = (age_secs / AGE_MATURITY_SECS).min(1.0) * AGE_MAX_BONUS;
    (origin_base_score(node.origin) + corroboration_bonus + age_bonus).clamp(MIN_TRUST_SCORE, 1.0)
}

#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("knowledge graph error: {0}")]
    Graph(#[from] GraphError),
}

/// docs/06 §Algorithms 1's outcome: a mention resolves cleanly, resolves
/// ambiguously (escalate rather than guess — §Recovery Mechanisms), or
/// matches nothing.
#[derive(Debug, Clone)]
pub enum EntityResolution {
    Resolved { node_id: NodeId, confidence: f32 },
    Ambiguous(Vec<NodeId>),
    NotFound,
}

/// One explained entry — docs/06 §Interfaces' `explain(bundle_id) ->
/// ProvenanceReport`, joining this crate's own ranking signals with
/// [09 — Knowledge Graph](../09-knowledge-graph.md)'s provenance chain.
#[derive(Debug, Clone)]
pub struct ExplainedEntry {
    pub node_id: NodeId,
    pub relevance_score: f32,
    pub source_signal: Vec<String>,
    pub provenance: ProvenanceChain,
}

const DISAMBIGUATION_FLOOR: f32 = 0.6;
const NOT_FOUND_FLOOR: f32 = 0.3;
const AMBIGUITY_MARGIN: f32 = 0.15;
// Tuned against this crate's own scoring weights below: a freshly-touched
// anchor (distance 0, full recency, no working-set history yet) scores
// ~0.55, which must land as `Full` — docs/06 §2's "a ticket title... [is]
// inlined" example is exactly this case.
const FULL_THRESHOLD: f32 = 0.5;
const SUMMARY_THRESHOLD: f32 = 0.25;
const SMALL_ENTRY_TOKENS: usize = 150;
const TRAVERSAL_DEPTH: usize = 2;

/// docs/06 — Context Engine, scored against a real
/// [`hyperion_knowledge_graph::KnowledgeGraph`] but fed a caller-constructed
/// [`Scope`] in place of a real Intent — see this crate's doc comment.
pub struct ContextEngine {
    graph: Arc<KnowledgeGraph>,
    working_sets: Mutex<HashMap<String, WorkingSet>>,
    next_bundle_id: AtomicU64,
    /// `None` by default: [`Self::summarize`] falls back to its own truncation stand-in — see
    /// [`Self::new_with_ai_runtime`]'s own doc comment for the real path this unlocks.
    ai_runtime: Option<Arc<LocalAiRuntime>>,
}

/// Matches `hyperion-agent-runtime::AgentRuntime::dispatch_document_draft`'s own precedent for
/// one real, resident `Slm`-class inference call -- generous enough that a real, modest-
/// throughput resident variant (see `hyperion_ai_runtime::CapabilityContract::
/// meets_latency_budget`'s own ~100-token-response proxy) still passes tier selection, without
/// letting one `summary`-mode entry's real inference call stall `assemble()` indefinitely.
const SUMMARIZE_LATENCY_BUDGET_MS: u64 = 15_000;

impl ContextEngine {
    pub fn new(graph: Arc<KnowledgeGraph>) -> Self {
        ContextEngine {
            graph,
            working_sets: Mutex::new(HashMap::new()),
            next_bundle_id: AtomicU64::new(1),
            ai_runtime: None,
        }
    }

    /// As [`Self::new`], but wires docs/06 §2's real `summary` inclusion mode this crate's own
    /// doc comment previously named as blocked on "Phase 3's Local AI Runtime" — which now
    /// exists. [`Self::summarize`] uses `ai_runtime` to produce a real, model-generated summary
    /// of an entry's metadata rather than truncating it to the first few fields; a caller not
    /// yet ready to wire this (or one whose token lacks the exec rights `LocalAiRuntime::infer`
    /// requires) keeps the exact same truncation behavior via [`Self::new`].
    pub fn new_with_ai_runtime(
        graph: Arc<KnowledgeGraph>,
        ai_runtime: Arc<LocalAiRuntime>,
    ) -> Self {
        ContextEngine {
            graph,
            working_sets: Mutex::new(HashMap::new()),
            next_bundle_id: AtomicU64::new(1),
            ai_runtime: Some(ai_runtime),
        }
    }

    fn estimate_tokens(value: &serde_json::Value) -> usize {
        serde_json::to_string(value)
            .map(|s| s.len() / 4)
            .unwrap_or(0)
    }

    /// docs/06 §2's `summary` inclusion mode: a real, model-generated summary when
    /// [`Self::new_with_ai_runtime`] wired an `ai_runtime`, falling back to the previous
    /// truncate-to-first-few-fields stand-in when it didn't, when this `token` isn't authorized
    /// for real inference (`RuntimeError::Unauthorized`), or when nothing is resident locally
    /// for `ModelClass::Slm` (`RuntimeError::InfeasibleLocally`) — a caller loses fidelity, not
    /// the whole bundle, exactly like this method's own caller already tolerates one
    /// unreachable anchor without failing the rest of `assemble()`.
    fn summarize(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        metadata: &serde_json::Value,
    ) -> serde_json::Value {
        if let Some(ai_runtime) = &self.ai_runtime {
            let request = InferenceRequest {
                prompt: format!(
                    "Summarize the following JSON object in one short sentence, keeping only \
                     its most salient fields:\n{metadata}"
                ),
            };
            let contract = CapabilityContract {
                latency_budget_ms: SUMMARIZE_LATENCY_BUDGET_MS,
                always_on: false,
            };
            if let Ok(result) =
                ai_runtime.infer(monitor, token, ModelClass::Slm, &contract, &request)
            {
                return serde_json::Value::String(result.text);
            }
        }
        Self::truncate(metadata)
    }

    /// The stand-in [`Self::summarize`] falls back to when no real `ai_runtime` is wired, or a
    /// real inference call couldn't run — keeps only the first few fields of an object's
    /// metadata rather than computing a real summary.
    fn truncate(metadata: &serde_json::Value) -> serde_json::Value {
        match metadata {
            serde_json::Value::Object(map) => serde_json::Value::Object(
                map.iter()
                    .take(3)
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    fn graph_distance_score(distance: Option<usize>) -> f32 {
        match distance {
            None => 0.3,
            Some(d) if d <= 2 => 1.0 / (1.0 + d as f32),
            Some(_) => 0.05,
        }
    }

    fn recency_score(updated_at: u64, now: u64) -> f32 {
        let age_hours = now.saturating_sub(updated_at) as f32 / 3600.0;
        1.0 / (1.0 + age_hours)
    }

    /// `ContextEngine.assemble` — docs/06 §Algorithms/§Pseudocode: collect
    /// candidates (working set + anchor traversal + resolved mentions),
    /// exclude anything outside the caller's Trust Boundary *before*
    /// scoring (§Security Considerations — never merely down-ranked), rank,
    /// and assemble under a hard token budget with graceful `full` ->
    /// `summary` -> `reference` degradation.
    pub fn assemble(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        scope: &Scope,
        budget: Budget,
    ) -> Result<ContextBundle, ContextError> {
        let mut working_sets = self.working_sets.lock().unwrap();
        let working_set = working_sets.entry(scope.session_id.clone()).or_default();

        let mut distances: HashMap<NodeId, usize> = HashMap::new();
        // docs/06 §Architecture's "Intent history" signal collector:
        // `hyperion-intent` stores every Intent as a real node in this
        // same graph, so the Intent actually driving this assembly is
        // itself a real anchor, not just an inert label on `scope` — its
        // own recent history (parent/sibling/dependency neighbors)
        // becomes part of the candidate pool exactly like an explicit
        // anchor would. `intent_id` is only ever traversed from if it
        // both parses as a real `NodeId` and genuinely names a node in
        // this graph — callers (including every existing test) that pass
        // an opaque placeholder string see no behavior change.
        let intent_anchor: Option<NodeId> = scope
            .intent_id
            .parse::<u64>()
            .ok()
            .map(ObjectId)
            .filter(|&id| self.graph.get(monitor, token, id).is_ok());
        for &anchor in scope.anchors.iter().chain(intent_anchor.iter()) {
            // An anchor this caller's Trust Boundary can't see -- a real object owned
            // elsewhere, or one that genuinely doesn't exist, indistinguishable to a caller
            // by design (see hyperion-knowledge-graph::traverse's own doc comment) --
            // contributes nothing to the pool rather than failing the whole assembly: one
            // unreachable anchor among several must never take down every other anchor's
            // real contribution.
            let subgraph = match self
                .graph
                .traverse(monitor, token, anchor, None, TRAVERSAL_DEPTH)
            {
                Ok(subgraph) => subgraph,
                Err(GraphError::NotFound) => continue,
                Err(e) => return Err(e.into()),
            };
            distances.entry(anchor).or_insert(0);
            for (node_id, _, depth) in subgraph.nodes {
                distances
                    .entry(node_id)
                    .and_modify(|d| *d = (*d).min(depth))
                    .or_insert(depth);
            }
        }

        let mut explicit_mentions: HashSet<NodeId> = HashSet::new();
        for mention in &scope.mentions {
            if let EntityResolution::Resolved { node_id, .. } =
                self.resolve_entity_locked(monitor, token, mention, working_set)?
            {
                explicit_mentions.insert(node_id);
                distances.entry(node_id).or_insert(0);
            }
        }

        let mut candidates: HashSet<NodeId> = working_set.active_node_ids().collect();
        candidates.extend(distances.keys().copied());
        candidates.extend(explicit_mentions.iter().copied());

        let now_ts = now();
        let caller_boundary = token.origin().0;

        let mut scored: Vec<(NodeId, f32, hyperion_knowledge_graph::NodeRecord)> = Vec::new();
        for node_id in candidates {
            let node = match self.graph.get(monitor, token, node_id) {
                Ok(n) => n,
                Err(GraphError::NotFound) => continue,
                Err(e) => return Err(e.into()),
            };
            // docs/06 §Security Considerations: excluded from the pool
            // entirely, never merely down-ranked.
            if node.owner != caller_boundary {
                continue;
            }

            let recency = Self::recency_score(node.updated_at, now_ts);
            let explicit = if explicit_mentions.contains(&node_id) {
                1.0
            } else {
                0.0
            };
            let graph_distance = Self::graph_distance_score(distances.get(&node_id).copied());
            let interaction_frequency = working_set.interaction_frequency(node_id);
            let base_score = 0.35 * recency
                + 0.35 * explicit
                + 0.20 * graph_distance
                + 0.10 * interaction_frequency
                + working_set.hysteresis_bonus(node_id);
            // docs/17 T4's own mitigation: "Context retrieval weights candidate objects by
            // Provenance Trust Score so an untrusted-origin object cannot silently outrank a
            // corroborated one." Multiplicative, not additive -- this demotes a low-trust
            // candidate relative to a well-attested one without ever re-deriving (and risking
            // detuning) this method's own already-tuned `base_score` weights or thresholds, and
            // `kg_trust_score`'s own real floor means this only ever demotes, never excludes,
            // matching this method's own "excluded entirely, never merely down-ranked" convention
            // being reserved for real Trust Boundary violations, not provenance distrust.
            let score = base_score * kg_trust_score(&node, now_ts);
            scored.push((node_id, score, node));
        }
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut entries = Vec::new();
        let mut tokens_used = 0usize;
        let mut category_counts: HashMap<String, usize> = HashMap::new();

        for (node_id, score, node) in scored {
            let category = node.object_type.clone();
            let count = category_counts.entry(category.clone()).or_insert(0);
            if *count >= budget.max_entries_per_category {
                continue;
            }

            let full_tokens = Self::estimate_tokens(&node.metadata);
            let (mode, content, cost) =
                if score > FULL_THRESHOLD && full_tokens <= SMALL_ENTRY_TOKENS {
                    (InclusionMode::Full, node.metadata.clone(), full_tokens)
                } else if score > SUMMARY_THRESHOLD {
                    let summary = self.summarize(monitor, token, &node.metadata);
                    let cost = Self::estimate_tokens(&summary).min(60);
                    (InclusionMode::Summary, summary, cost)
                } else {
                    (InclusionMode::Reference, serde_json::Value::Null, 5)
                };

            if tokens_used + cost > budget.max_tokens {
                break; // hard bound, never exceeded — docs/06 §Algorithms 2
            }

            let mut source_signal = Vec::new();
            if explicit_mentions.contains(&node_id) {
                source_signal.push("explicit_mention".to_string());
            }
            if distances.get(&node_id).copied() == Some(0) {
                source_signal.push("anchor".to_string());
            } else if distances.contains_key(&node_id) {
                source_signal.push("graph_traversal".to_string());
            }
            if working_set.interaction_frequency(node_id) > 0.0 {
                source_signal.push("working_set".to_string());
            }

            *count += 1;
            tokens_used += cost;
            working_set.record_inclusion(node_id, now_ts);

            entries.push(ContextEntry {
                category,
                node_id,
                inclusion_mode: mode,
                content,
                relevance_score: score,
                source_signal,
                generation: node.updated_at,
                captured_at: now_ts,
            });
        }

        Ok(ContextBundle {
            bundle_id: self.next_bundle_id.fetch_add(1, Ordering::Relaxed),
            scope: scope.clone(),
            entries,
            assembled_at: now_ts,
            budget,
            expertise_signal: working_set.expertise_estimate("general"),
        })
    }

    /// `ContextEngine.resolveEntity` — docs/06 §Algorithms 1. Intersects a
    /// fuzzy text match against node metadata with the session's working
    /// set; escalates (returns [`EntityResolution::Ambiguous`]) rather than
    /// guessing when candidates are close and both below the
    /// disambiguation floor, per docs/06 §Recovery Mechanisms.
    pub fn resolve_entity(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        mention: &str,
        session_id: &str,
    ) -> Result<EntityResolution, ContextError> {
        let mut working_sets = self.working_sets.lock().unwrap();
        let working_set = working_sets.entry(session_id.to_string()).or_default();
        self.resolve_entity_locked(monitor, token, mention, working_set)
    }

    fn resolve_entity_locked(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        mention: &str,
        working_set: &WorkingSet,
    ) -> Result<EntityResolution, ContextError> {
        let hits = self.graph.query(
            monitor,
            token,
            &GraphQuery {
                limit: 200,
                ..Default::default()
            },
        )?;

        let needle = mention.to_lowercase();
        // docs/06 §Algorithms 1: intersect the fuzzy text match with (b)
        // recency in the session's working memory — a candidate already
        // active in this session is more likely to be what "the API" means
        // than an equally-fuzzy match the user hasn't touched all session.
        let mut scored: Vec<(NodeId, f32)> = hits
            .into_iter()
            .filter(|h| h.node.owner == token.origin().0)
            .filter_map(|h| {
                let text_score = Self::fuzzy_match_score(&needle, &h.node.metadata);
                if text_score <= 0.0 {
                    return None;
                }
                let affinity = working_set.interaction_frequency(h.node_id) * 0.15;
                Some((h.node_id, (text_score + affinity).min(1.0)))
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        match scored.first() {
            None => Ok(EntityResolution::NotFound),
            Some((_, top)) if *top < NOT_FOUND_FLOOR => Ok(EntityResolution::NotFound),
            Some((top_id, top_score)) => {
                let runner_up = scored.get(1).map(|(_, s)| *s).unwrap_or(0.0);
                if *top_score < DISAMBIGUATION_FLOOR && (top_score - runner_up) < AMBIGUITY_MARGIN {
                    let tied: Vec<NodeId> = scored
                        .iter()
                        .filter(|(_, s)| (top_score - s).abs() < AMBIGUITY_MARGIN)
                        .map(|(id, _)| *id)
                        .collect();
                    Ok(EntityResolution::Ambiguous(tied))
                } else {
                    Ok(EntityResolution::Resolved {
                        node_id: *top_id,
                        confidence: *top_score,
                    })
                }
            }
        }
    }

    fn fuzzy_match_score(needle: &str, metadata: &serde_json::Value) -> f32 {
        let haystacks: Vec<String> = match metadata {
            serde_json::Value::Object(map) => map
                .values()
                .filter_map(|v| v.as_str().map(|s| s.to_lowercase()))
                .collect(),
            _ => Vec::new(),
        };
        haystacks
            .iter()
            .map(|h| {
                if h == needle {
                    1.0
                } else if h.contains(needle) || needle.contains(h.as_str()) {
                    0.75
                } else {
                    let needle_words: HashSet<&str> = needle.split_whitespace().collect();
                    let haystack_words: HashSet<&str> = h.split_whitespace().collect();
                    let overlap = needle_words.intersection(&haystack_words).count();
                    if overlap == 0 || needle_words.is_empty() {
                        0.0
                    } else {
                        overlap as f32 / needle_words.len().max(haystack_words.len()) as f32
                    }
                }
            })
            .fold(0.0_f32, f32::max)
    }

    /// `ContextEngine.expand` — docs/06 §Interfaces: lazy fetch for
    /// `reference`/`summary` entries an Agent needs the full object for.
    pub fn expand(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        entry: &ContextEntry,
    ) -> Result<serde_json::Value, ContextError> {
        Ok(self.graph.get(monitor, token, entry.node_id)?.metadata)
    }

    /// `ContextEngine.explain` — docs/06 §Interfaces.
    pub fn explain(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        bundle: &ContextBundle,
    ) -> Result<Vec<ExplainedEntry>, ContextError> {
        bundle
            .entries
            .iter()
            .map(|entry| {
                let provenance = self.graph.explain(
                    monitor,
                    token,
                    hyperion_knowledge_graph::ExplainRef::Node(entry.node_id),
                )?;
                Ok(ExplainedEntry {
                    node_id: entry.node_id,
                    relevance_score: entry.relevance_score,
                    source_signal: entry.source_signal.clone(),
                    provenance,
                })
            })
            .collect()
    }

    /// `ContextEngine.currentExpertise` — see this crate's doc comment's
    /// Adaptive Complexity deferral: a real signal derived from `session_id`'s
    /// own working-set activity, narrower than docs/06's fuller vocabulary-
    /// complexity/capability-tier read, honestly labeled as such rather than
    /// fabricated. A session with no working-set activity yet (never
    /// assembled a bundle, or an unrecognized `session_id`) reports the same
    /// fixed, zero-confidence `Novice` estimate this method always returned
    /// before.
    pub fn current_expertise(&self, session_id: &str, domain: &str) -> ExpertiseEstimate {
        let working_sets = self.working_sets.lock().unwrap();
        match working_sets.get(session_id) {
            Some(working_set) => working_set.expertise_estimate(domain),
            None => ExpertiseEstimate {
                domain: domain.to_string(),
                level: ExpertiseLevel::Novice,
                evidence: vec!["no working-set activity yet for this session".to_string()],
                confidence: 0.0,
            },
        }
    }

    /// This crate's own previously-named "Adaptive Complexity" gap, closed for real: pushes one
    /// real, already-computed [`ExpertiseSignal`] sample for `session_id`, so [`Self::
    /// current_expertise`]'s next call blends it in -- see [`ExpertiseSignal`]'s own doc comment
    /// for why this crate never computes these three signals itself, and which real callers
    /// (`hyperion-intent`, `hyperion-console`) push them. Creates `session_id`'s own working set
    /// if this is the very first real activity recorded for it, the same "first touch creates
    /// the entry" convention [`Self::resolve_entity`] already established.
    pub fn record_expertise_signal(&self, session_id: &str, signal: ExpertiseSignal) {
        let mut working_sets = self.working_sets.lock().unwrap();
        working_sets
            .entry(session_id.to_string())
            .or_default()
            .record_expertise_signal(signal);
    }
}
