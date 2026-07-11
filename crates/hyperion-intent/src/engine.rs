use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use hyperion_capability::{CapabilityMonitor, CapabilityToken};
use hyperion_context::{ContextEngine, ContextError, EntityResolution};
use hyperion_explainability::{
    ControlState, ExplanationId, ExplanationRecord, ExplanationStore, ReasoningStep,
};
use hyperion_knowledge_graph::{
    EdgeOrigin, ExplainRef, GraphError, KnowledgeGraph, NodeId, ProvenanceChain,
};
use hyperion_memory::WorkingMemory;

use crate::templates::{self, Template};
use crate::types::{ExecutionTicket, HandleOutcome, Intent, IntentStatus, MutationOp};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs()
}

/// docs/05 §4's ask/infer disambiguation floor, reused directly from
/// `hyperion-context`'s own [`EntityResolution`] outcome rather than
/// re-implemented — grounding is one shared concern, not two.
const URGENCY_KEYWORDS: [&str; 3] = ["urgent", "asap", "now"];
const CANCEL_KEYWORDS: [&str; 4] = ["cancel", "stop", "forget it", "never mind"];

#[derive(Debug, thiserror::Error)]
pub enum IntentError {
    #[error("knowledge graph error: {0}")]
    Graph(#[from] GraphError),
    #[error("context engine error: {0}")]
    Context(#[from] ContextError),
    #[error("no such intent")]
    NotFound,
    #[error("that mutation would introduce a dependency cycle")]
    CyclicDependency,
    #[error("explainability error: {0}")]
    Explainability(#[from] hyperion_explainability::ExplainabilityError),
}

/// docs/08 §4's own working-memory sizing is caller-defined; ten recent
/// turns is a hosted-simulator-appropriate default, not a value docs/05
/// or docs/08 pin down.
const WORKING_MEMORY_TURN_CAPACITY: usize = 10;

/// docs/05 — Intent Engine. See this crate's doc comment for what's
/// deferred.
pub struct IntentEngine {
    graph: Arc<KnowledgeGraph>,
    context: Arc<ContextEngine>,
    /// Per-session stack of root ids, most-recently-touched last — the
    /// narrowed reference-resolution target for reconciliation (§5); see
    /// this crate's doc comment.
    active_graphs: Mutex<HashMap<String, Vec<NodeId>>>,
    /// docs/05 §7's Memory Engine integration: every real
    /// `hyperion_memory::WorkingMemory` turn buffer this engine has
    /// pushed an utterance into, keyed by session — see this crate's doc
    /// comment on what's still deferred (using these turns as a real
    /// grounding signal, not just recording them).
    working_memories: Mutex<HashMap<String, WorkingMemory>>,
    /// docs/18's Explanation Record store for this engine's own real
    /// decision point — HTN decomposition (see [`Self::handle_utterance`])
    /// — see [`Self::explanation`]/[`Self::trace_intent`].
    explanations: ExplanationStore,
    next_action_id: AtomicU64,
}

impl IntentEngine {
    pub fn new(graph: Arc<KnowledgeGraph>, context: Arc<ContextEngine>) -> Self {
        IntentEngine {
            graph,
            context,
            active_graphs: Mutex::new(HashMap::new()),
            working_memories: Mutex::new(HashMap::new()),
            explanations: ExplanationStore::new(),
            next_action_id: AtomicU64::new(1),
        }
    }

    /// docs/18's "queryable Explanation Record" surface for this engine's
    /// own decomposition dispatches — see [`Self::handle_utterance`].
    pub fn explanation(&self, id: ExplanationId) -> Option<ExplanationRecord> {
        self.explanations.get(id)
    }

    /// Every record this engine has opened for real Intent `intent_id` —
    /// unlike `hyperion-coordination`/`hyperion-federation`'s own stores,
    /// this engine mints the real Intent id itself, so this is a real
    /// correlation, not a sentinel.
    pub fn trace_intent(&self, intent_id: u64) -> Vec<ExplanationRecord> {
        self.explanations.trace_intent(intent_id)
    }

    /// The real `hyperion-memory` turn buffer for `session_id`, if this
    /// engine has ever handled an utterance for it — queryable proof
    /// [`Self::handle_utterance`] genuinely records into a real
    /// `WorkingMemory`, not a private, parallel buffer.
    pub fn working_memory_turns(&self, session_id: &str) -> Vec<String> {
        self.working_memories
            .lock()
            .unwrap()
            .get(session_id)
            .map(|wm| wm.turns().map(str::to_string).collect())
            .unwrap_or_default()
    }

    fn put_intent(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        existing_id: Option<NodeId>,
        intent: &Intent,
    ) -> Result<NodeId, IntentError> {
        let metadata = serde_json::to_value(intent).expect("Intent always serializes");
        Ok(self
            .graph
            .put_node(monitor, token, existing_id, "intent", None, metadata)?)
    }

    fn get_intent(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: NodeId,
    ) -> Result<Intent, IntentError> {
        let node = self.graph.get(monitor, token, id)?;
        let mut intent: Intent =
            serde_json::from_value(node.metadata).map_err(|_| IntentError::NotFound)?;
        intent.id = id;
        Ok(intent)
    }

    fn extract_mention(utterance: &str) -> Option<String> {
        let lower = utterance.to_lowercase();
        for marker in [" on ", " about ", " regarding "] {
            if let Some(pos) = lower.find(marker) {
                let tail = utterance[pos + marker.len()..].trim();
                if !tail.is_empty() {
                    return Some(tail.trim_end_matches('.').to_string());
                }
            }
        }
        None
    }

    /// docs/05 §Pseudocode `handle_utterance` — reconciliation first
    /// (§Algorithms 5), then parse/ground/decompose/prioritize for a fresh
    /// Intent Graph.
    pub fn handle_utterance(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        utterance: &str,
        session_id: &str,
    ) -> Result<HandleOutcome, IntentError> {
        self.working_memories
            .lock()
            .unwrap()
            .entry(session_id.to_string())
            .or_insert_with(|| WorkingMemory::new(session_id, WORKING_MEMORY_TURN_CAPACITY))
            .push_turn(utterance);

        let lower = utterance.to_lowercase();
        if CANCEL_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
            let active_root = self
                .active_graphs
                .lock()
                .unwrap()
                .get(session_id)
                .and_then(|v| v.last())
                .copied();
            if let Some(root) = active_root {
                self.apply_mutation(monitor, token, root, MutationOp::Cancel)?;
                return Ok(HandleOutcome::Submitted(root));
            }
        }

        let template = templates::match_template(utterance);
        let now_ts = now();
        let urgent = URGENCY_KEYWORDS.iter().any(|kw| lower.contains(kw));

        let mut grounded_entities = Vec::new();
        let mut inferred_fields = Vec::new();
        if template.is_none() {
            if let Some(mention) = Self::extract_mention(utterance) {
                match self
                    .context
                    .resolve_entity(monitor, token, &mention, session_id)?
                {
                    EntityResolution::Resolved { node_id, .. } => {
                        grounded_entities.push(node_id);
                        inferred_fields.push(mention);
                    }
                    EntityResolution::Ambiguous(candidates) => {
                        return Ok(HandleOutcome::NeedsClarification {
                            mention,
                            candidates,
                        });
                    }
                    EntityResolution::NotFound => {
                        // Degrade, never block — docs/02 §4 invariant 5.
                        // The Intent proceeds ungrounded.
                    }
                }
            }
        }

        let (predicate, confidence, root_status) = match template {
            Some(t) => (t.root_predicate.to_string(), 0.9, IntentStatus::Planned),
            None => ("generic_goal".to_string(), 0.3, IntentStatus::Proposed),
        };

        let mut root_intent = Intent {
            id: hyperion_storage::ObjectId(0),
            raw_utterance: utterance.to_string(),
            predicate,
            status: root_status,
            priority: 1.0,
            confidence,
            parent: None,
            children: Vec::new(),
            grounded_entities,
            inferred_fields,
            version: 1,
            created_at: now_ts,
            updated_at: now_ts,
        };
        let root = self.put_intent(monitor, token, None, &root_intent)?;

        if let Some(t) = template {
            let action_id = self.next_action_id.fetch_add(1, Ordering::Relaxed);
            let explanation_id = self.explanations.begin(
                monitor,
                token,
                action_id,
                root.0,
                0,
                &root_intent.predicate,
                vec![],
                now_ts,
            )?;

            let children = self.decompose(monitor, token, root, t, urgent, now_ts)?;

            for (i, (&leaf_id, leaf)) in children.iter().zip(t.leaves.iter()).enumerate() {
                self.explanations.append_step(
                    monitor,
                    token,
                    explanation_id,
                    ReasoningStep {
                        step_index: i as u32,
                        description: format!("decomposed into '{}'", leaf.predicate),
                        capability_ref: Some(leaf.predicate.to_string()),
                        inputs_ref: vec![root],
                        output_ref: Some(leaf_id),
                    },
                    Vec::new(),
                )?;
            }
            self.explanations.transition(
                monitor,
                token,
                explanation_id,
                ControlState::Completed,
            )?;

            root_intent.id = root;
            root_intent.children = children;
            self.put_intent(monitor, token, Some(root), &root_intent)?;
        }

        self.active_graphs
            .lock()
            .unwrap()
            .entry(session_id.to_string())
            .or_default()
            .push(root);
        Ok(HandleOutcome::Submitted(root))
    }

    /// docs/05 §Algorithms 2/3: flat HTN decomposition (see this crate's
    /// doc comment on the nested-subtree deferral) plus derived priority —
    /// dependency depth discounts priority, explicit urgency language
    /// boosts it.
    fn decompose(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        root: NodeId,
        template: &Template,
        urgent: bool,
        now_ts: u64,
    ) -> Result<Vec<NodeId>, IntentError> {
        let mut leaf_ids = Vec::with_capacity(template.leaves.len());
        for leaf in template.leaves {
            let ready = leaf.depends_on.is_empty();
            let status = if ready {
                IntentStatus::Executing
            } else {
                IntentStatus::Planned
            };
            let priority = (0.9 - 0.05 * leaf.depends_on.len() as f32
                + if urgent { 0.1 } else { 0.0 })
            .min(1.0);
            let intent = Intent {
                id: hyperion_storage::ObjectId(0),
                raw_utterance: String::new(),
                predicate: leaf.predicate.to_string(),
                status,
                priority,
                confidence: 0.9,
                parent: Some(root),
                children: Vec::new(),
                grounded_entities: Vec::new(),
                inferred_fields: Vec::new(),
                version: 0,
                created_at: now_ts,
                updated_at: now_ts,
            };
            leaf_ids.push(self.put_intent(monitor, token, None, &intent)?);
        }
        for (i, leaf) in template.leaves.iter().enumerate() {
            for &dep_idx in leaf.depends_on {
                self.graph.link(
                    monitor,
                    token,
                    leaf_ids[i],
                    "depends_on",
                    leaf_ids[dep_idx],
                    1.0,
                    EdgeOrigin::Explicit,
                    None,
                    "htn_template",
                    None,
                )?;
            }
        }
        Ok(leaf_ids)
    }

    /// docs/05 §Pseudocode `apply_mutation`. `Amend`/`Supersede` are
    /// exposed as their own focused methods
    /// ([`Self::add_dependency`]) rather than through this generic
    /// dispatcher — see this crate's doc comment.
    fn apply_mutation(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        target: NodeId,
        op: MutationOp,
    ) -> Result<(), IntentError> {
        match op {
            MutationOp::Cancel => {
                let intent = self.get_intent(monitor, token, target)?;
                let root = intent.parent.unwrap_or(target);
                self.abandon_subtree(monitor, token, target)?;
                self.bump_version(monitor, token, root)?;
                Ok(())
            }
            MutationOp::Amend | MutationOp::Supersede => Ok(()), // not reached via this dispatcher yet
        }
    }

    fn abandon_subtree(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        target: NodeId,
    ) -> Result<(), IntentError> {
        let mut intent = self.get_intent(monitor, token, target)?;
        intent.status = IntentStatus::Abandoned;
        intent.updated_at = now();
        let children = intent.children.clone();
        self.put_intent(monitor, token, Some(target), &intent)?;
        for child in children {
            self.abandon_subtree(monitor, token, child)?;
        }
        Ok(())
    }

    fn bump_version(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        root: NodeId,
    ) -> Result<(), IntentError> {
        let mut intent = self.get_intent(monitor, token, root)?;
        intent.version += 1;
        intent.updated_at = now();
        self.put_intent(monitor, token, Some(root), &intent)?;
        Ok(())
    }

    /// Every outgoing `depends_on` edge from `node` — docs/05 §4's
    /// `TaskNode.dependencies` equivalent, public so
    /// `hyperion-coordination` (or any other consumer building a task
    /// graph from an Intent Graph) can read prerequisite structure without
    /// this crate exposing raw graph edges. Built on
    /// [`hyperion_knowledge_graph::KnowledgeGraph::explain`] rather than a
    /// new lower-level graph API, since `explain` already exposes exactly
    /// the (subject, predicate, target) triples a directed walk needs.
    pub fn depends_on_targets(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        node: NodeId,
    ) -> Result<Vec<NodeId>, IntentError> {
        let ProvenanceChain::Node { incident_edges, .. } =
            self.graph.explain(monitor, token, ExplainRef::Node(node))?
        else {
            unreachable!("explain(Node(_)) always returns ProvenanceChain::Node");
        };
        let mut targets = Vec::new();
        for edge_id in incident_edges {
            if let ProvenanceChain::Edge {
                subject,
                predicate,
                target,
                tombstone,
                ..
            } = self
                .graph
                .explain(monitor, token, ExplainRef::Edge(edge_id))?
            {
                if !tombstone && subject == node && predicate == "depends_on" {
                    targets.push(target);
                }
            }
        }
        Ok(targets)
    }

    fn would_create_cycle(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        from: NodeId,
        to: NodeId,
    ) -> Result<bool, IntentError> {
        let mut visited = HashSet::new();
        let mut stack = vec![to];
        while let Some(node) = stack.pop() {
            if node == from {
                return Ok(true);
            }
            if !visited.insert(node) {
                continue;
            }
            stack.extend(self.depends_on_targets(monitor, token, node)?);
        }
        Ok(false)
    }

    /// docs/05 §Failure Modes: "a cycle introduced by a bad `amend`...
    /// detected at commit time and rejected before `persist()`."
    pub fn add_dependency(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        from: NodeId,
        to: NodeId,
    ) -> Result<(), IntentError> {
        if from == to || self.would_create_cycle(monitor, token, from, to)? {
            return Err(IntentError::CyclicDependency);
        }
        self.graph.link(
            monitor,
            token,
            from,
            "depends_on",
            to,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "amend",
            None,
        )?;
        Ok(())
    }

    /// docs/05 §Interfaces' `getGraph` — this crate's flat model, so
    /// "the graph" is just the root plus its direct children.
    pub fn get_graph(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        root: NodeId,
    ) -> Result<Vec<Intent>, IntentError> {
        let root_intent = self.get_intent(monitor, token, root)?;
        let mut all = vec![root_intent.clone()];
        for child in &root_intent.children {
            all.push(self.get_intent(monitor, token, *child)?);
        }
        Ok(all)
    }

    /// docs/05 §Interfaces' `explain`.
    pub fn explain(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: NodeId,
    ) -> Result<ProvenanceChain, IntentError> {
        Ok(self.graph.explain(monitor, token, ExplainRef::Node(id))?)
    }

    /// docs/05 §Interfaces' `submit` — see this crate's doc comment: no
    /// real Multi-Agent Coordination exists yet to hand this off to.
    pub fn submit(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        root: NodeId,
    ) -> Result<ExecutionTicket, IntentError> {
        let graph = self.get_graph(monitor, token, root)?;
        let ready_leaves = graph
            .into_iter()
            .filter(|i| i.status == IntentStatus::Executing)
            .map(|i| i.id)
            .collect();
        Ok(ExecutionTicket { root, ready_leaves })
    }
}
