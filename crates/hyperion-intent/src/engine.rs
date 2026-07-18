use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use hyperion_ai_runtime::{CapabilityContract, InferenceRequest, LocalAiRuntime, ModelClass};
use hyperion_capability::{CapabilityMonitor, CapabilityToken};
use hyperion_context::{ContextEngine, ContextError, EntityResolution};
use hyperion_explainability::{
    ControlState, ExplanationId, ExplanationRecord, ExplanationStore, ReasoningStep,
};
use hyperion_knowledge_graph::{
    EdgeOrigin, ExplainRef, GraphError, KnowledgeGraph, NodeId, ProvenanceChain,
};
use hyperion_memory::WorkingMemory;
use hyperion_plugin_framework::PluginRegistry;

use crate::templates::{self, Template, TemplateLeaf};
use crate::types::{ExecutionTicket, HandleOutcome, Intent, IntentStatus, MutationOp};

/// Turns one line of a model-generated plan into a terse `snake_case`-shaped predicate --
/// collapsing runs of whitespace/punctuation into single underscores -- so a real model response
/// (which won't reliably already be in that shape) still produces a usable
/// [`TemplateLeaf::predicate`] exactly like this crate's own curated templates already are.
fn to_predicate(line: &str) -> String {
    line.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

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

/// `hyperion-context::ContextEngine`'s own `SUMMARIZE_LATENCY_BUDGET_MS` precedent for one real,
/// resident `Slm`-class inference call -- generous enough that a real, modest-throughput resident
/// variant still passes tier selection, without letting a single unmatched utterance stall
/// [`IntentEngine::handle_utterance`] indefinitely.
const GENERATE_TEMPLATE_LATENCY_BUDGET_MS: u64 = 15_000;

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
    /// docs/998-roadmap.md's Resourceful pillar: a real, optional `PluginRegistry` so
    /// [`Self::handle_utterance`]'s template match considers a plugin-contributed
    /// `Contribution::AutomationWorkflow` too, not just the built-in `templates::TEMPLATES`
    /// roster — the same optional-real-backend shape `AgentRuntime`/`CoordinationSession`
    /// already use for their own `Option<Arc<PluginRegistry>>`.
    plugins: Option<Arc<PluginRegistry>>,
    /// docs/998-roadmap.md's Backlog "Protect the Human" item: sessions opted into a real,
    /// human-owned pause before decomposition — see [`Self::set_think_mode`]. Purely local session
    /// state, like `active_graphs`; not a capability-gated action.
    think_mode_sessions: Mutex<HashSet<String>>,
    /// A matched template [`Self::handle_utterance`] deliberately withheld from decomposition
    /// because its session is in think mode, keyed by the pending root — see
    /// [`Self::proceed_with_decomposition`].
    pending_decompositions: Mutex<HashMap<NodeId, PendingDecomposition>>,
    /// This crate's own named "generative decomposition" gap: `None` by default, in which case
    /// `generate_template` never runs and an utterance matching no curated or
    /// plugin-contributed template keeps falling back to a single undecomposed root Intent --
    /// see [`Self::new_with_plugins_and_ai_runtime`]'s own doc comment for the real path this
    /// unlocks.
    ai_runtime: Option<Arc<LocalAiRuntime>>,
}

/// What [`IntentEngine::proceed_with_decomposition`] needs to finish what
/// [`IntentEngine::handle_utterance`] started once think mode is satisfied — exactly the
/// arguments `IntentEngine::decompose_and_record` takes beyond the root Intent itself.
struct PendingDecomposition {
    template: Template,
    urgent: bool,
    now_ts: u64,
}

impl IntentEngine {
    pub fn new(graph: Arc<KnowledgeGraph>, context: Arc<ContextEngine>) -> Self {
        Self::new_with_plugins(graph, context, None)
    }

    /// As [`Self::new`], additionally wiring a real [`PluginRegistry`] so a plugin-contributed
    /// goal template can really compete for a real utterance match.
    pub fn new_with_plugins(
        graph: Arc<KnowledgeGraph>,
        context: Arc<ContextEngine>,
        plugins: Option<Arc<PluginRegistry>>,
    ) -> Self {
        Self::new_with_plugins_and_ai_runtime(graph, context, plugins, None)
    }

    /// As [`Self::new_with_plugins`], additionally wiring a real [`LocalAiRuntime`] so
    /// [`Self::handle_utterance`]'s fallback path -- an utterance matching no curated or
    /// plugin-contributed template -- produces a real, model-generated ordered step list (this
    /// crate's own previously-named "generative decomposition" gap) instead of always degrading
    /// to a single undecomposed root Intent. See `generate_template` for the real
    /// generation and honest-fallback logic.
    pub fn new_with_plugins_and_ai_runtime(
        graph: Arc<KnowledgeGraph>,
        context: Arc<ContextEngine>,
        plugins: Option<Arc<PluginRegistry>>,
        ai_runtime: Option<Arc<LocalAiRuntime>>,
    ) -> Self {
        IntentEngine {
            graph,
            context,
            active_graphs: Mutex::new(HashMap::new()),
            working_memories: Mutex::new(HashMap::new()),
            explanations: ExplanationStore::new(),
            next_action_id: AtomicU64::new(1),
            plugins,
            ai_runtime,
            think_mode_sessions: Mutex::new(HashSet::new()),
            pending_decompositions: Mutex::new(HashMap::new()),
        }
    }

    /// Opts `session_id` into (or back out of) docs/998-roadmap.md's Backlog "Protect the Human"
    /// pause: while enabled, a matched template's decomposition in [`Self::handle_utterance`]
    /// waits for an explicit [`Self::proceed_with_decomposition`] call instead of happening
    /// immediately. Deliberately opt-in and per-session, matching that item's own explicit
    /// constraint — "not a default that adds friction to every goal."
    pub fn set_think_mode(&self, session_id: &str, enabled: bool) {
        let mut sessions = self.think_mode_sessions.lock().unwrap();
        if enabled {
            sessions.insert(session_id.to_string());
        } else {
            sessions.remove(session_id);
        }
    }

    /// Whether `session_id` is currently in think mode — see [`Self::set_think_mode`].
    pub fn is_think_mode(&self, session_id: &str) -> bool {
        self.think_mode_sessions
            .lock()
            .unwrap()
            .contains(session_id)
    }

    /// The real, explicit second step for a session in think mode: commits to decomposing `root`
    /// (the pending Intent [`Self::handle_utterance`] created but deliberately did not decompose)
    /// only once the caller — the human's own reasoning, not Hyperion's — actually asks it to
    /// proceed. [`IntentError::NotFound`] if `root` names no pending decomposition (already
    /// decided, or never paused to begin with).
    pub fn proceed_with_decomposition(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        root: NodeId,
    ) -> Result<HandleOutcome, IntentError> {
        let pending = self
            .pending_decompositions
            .lock()
            .unwrap()
            .remove(&root)
            .ok_or(IntentError::NotFound)?;
        let mut root_intent = self.get_intent(monitor, token, root)?;
        self.decompose_and_record(
            monitor,
            token,
            root,
            &mut root_intent,
            &pending.template,
            pending.urgent,
            pending.now_ts,
        )?;
        Ok(HandleOutcome::Submitted(root))
    }

    /// docs/18's "queryable Explanation Record" surface for this engine's
    /// own decomposition dispatches — see [`Self::handle_utterance`]. Capability-checked and
    /// Trust-Boundary-filtered (2026-07-16), threading straight through to
    /// `hyperion_explainability::ExplanationStore::get`'s own real gating — this thin wrapper
    /// previously re-exposed the same ungated hole one layer up.
    pub fn explanation(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: ExplanationId,
    ) -> Result<Option<ExplanationRecord>, IntentError> {
        Ok(self.explanations.get(monitor, token, id)?)
    }

    /// Every record this engine has opened for real Intent `intent_id` —
    /// unlike `hyperion-coordination`/`hyperion-federation`'s own stores,
    /// this engine mints the real Intent id itself, so this is a real
    /// correlation, not a sentinel. Capability-checked and Trust-Boundary-filtered (2026-07-16),
    /// the same way [`Self::explanation`] now is.
    pub fn trace_intent(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        intent_id: u64,
    ) -> Result<Vec<ExplanationRecord>, IntentError> {
        Ok(self.explanations.trace_intent(monitor, token, intent_id)?)
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

    /// docs/05 §2's fallback for a goal shape matching no curated or plugin-contributed HTN
    /// template: a real, model-generated ordered step list when [`Self::new_with_plugins_and_ai_runtime`]
    /// wired a real `ai_runtime`, in place of [`Self::handle_utterance`]'s previous
    /// single-undecomposed-root stand-in. Each non-empty line of the model's response becomes one
    /// [`TemplateLeaf`], depending on the line before it -- a real, if modest, ordered plan, not a
    /// fabricated dependency structure the model was never asked for. `None` -- degrading to the
    /// pre-existing fallback -- when no `ai_runtime` is wired, this `token` isn't authorized for
    /// real inference, nothing is resident locally for `ModelClass::Slm`, or the response yields no
    /// usable steps at all.
    fn generate_template(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        utterance: &str,
    ) -> Option<Template> {
        let ai_runtime = self.ai_runtime.as_ref()?;
        let request = InferenceRequest {
            prompt: format!(
                "Break the following goal down into a short ordered list of concrete steps, \
                 one per line, using terse snake_case labels and no numbering or punctuation:\
                 \n{utterance}"
            ),
        };
        let contract = CapabilityContract {
            latency_budget_ms: GENERATE_TEMPLATE_LATENCY_BUDGET_MS,
            always_on: false,
        };
        let result = ai_runtime
            .infer(monitor, token, ModelClass::Slm, &contract, &request)
            .ok()?;

        let leaves: Vec<TemplateLeaf> = result
            .text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(to_predicate)
            .filter(|predicate| !predicate.is_empty())
            .enumerate()
            .map(|(i, predicate)| TemplateLeaf {
                predicate,
                depends_on: if i == 0 { Vec::new() } else { vec![i - 1] },
            })
            .collect();

        if leaves.is_empty() {
            return None;
        }
        Some(Template {
            root_predicate: "generic_goal".to_string(),
            leaves,
        })
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

        // This crate's own real half of `hyperion-context`'s previously-named Adaptive
        // Complexity gap: this crate already depends on `hyperion-context` (the reverse edge
        // would be a real cycle), so it pushes a real, already-computed vocabulary-complexity
        // sample for every real utterance rather than `hyperion-context` ever reading this
        // crate's own utterances directly.
        self.context.record_expertise_signal(
            session_id,
            hyperion_context::ExpertiseSignal::VocabularyComplexity(
                hyperion_context::vocabulary_complexity(utterance),
            ),
        );

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

        let template = templates::match_template_with_plugins(utterance, self.plugins.as_deref());
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

        // This crate's own named "generative decomposition" gap: an utterance matching no
        // curated or plugin-contributed template gets one real, model-generated attempt before
        // falling all the way back to a single undecomposed root -- lower confidence than a
        // curated match (a model's own ordered guess, not a hand-authored plan), but a real plan
        // all the same.
        let generated_template = if template.is_none() {
            self.generate_template(monitor, token, utterance)
        } else {
            None
        };

        let (predicate, confidence, root_status) = match (&template, &generated_template) {
            (Some(t), _) => (t.root_predicate.clone(), 0.9, IntentStatus::Planned),
            (None, Some(t)) => (t.root_predicate.clone(), 0.6, IntentStatus::Planned),
            (None, None) => ("generic_goal".to_string(), 0.3, IntentStatus::Proposed),
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

        if let Some(t) = template.as_ref().or(generated_template.as_ref()) {
            if self.is_think_mode(session_id) {
                self.pending_decompositions.lock().unwrap().insert(
                    root,
                    PendingDecomposition {
                        template: t.clone(),
                        urgent,
                        now_ts,
                    },
                );
                self.active_graphs
                    .lock()
                    .unwrap()
                    .entry(session_id.to_string())
                    .or_default()
                    .push(root);
                return Ok(HandleOutcome::PendingThink(root));
            }
            self.decompose_and_record(monitor, token, root, &mut root_intent, t, urgent, now_ts)?;
        }

        self.active_graphs
            .lock()
            .unwrap()
            .entry(session_id.to_string())
            .or_default()
            .push(root);
        Ok(HandleOutcome::Submitted(root))
    }

    /// The decompose-then-explain-then-persist sequence shared by
    /// [`Self::handle_utterance`]'s immediate path and
    /// [`Self::proceed_with_decomposition`]'s deferred one — see this crate's doc comment on the
    /// think-mode pause.
    #[allow(clippy::too_many_arguments)]
    fn decompose_and_record(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        root: NodeId,
        root_intent: &mut Intent,
        template: &Template,
        urgent: bool,
        now_ts: u64,
    ) -> Result<(), IntentError> {
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

        let children = self.decompose(monitor, token, root, template, urgent, now_ts)?;

        for (i, (&leaf_id, leaf)) in children.iter().zip(template.leaves.iter()).enumerate() {
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
        self.explanations
            .transition(monitor, token, explanation_id, ControlState::Completed)?;

        root_intent.id = root;
        root_intent.children = children;
        self.put_intent(monitor, token, Some(root), root_intent)?;
        Ok(())
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
        for leaf in &template.leaves {
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
            for &dep_idx in &leaf.depends_on {
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

    /// docs/05 §6's own named "conflict detection across active graphs" prerequisite, made real:
    /// once `submit()` hands an Intent leaf's execution off, nothing in this crate's own real API
    /// surface ever transitions its `status` again -- `hyperion-coordination`'s own real dispatch
    /// pipeline (the one real caller with a genuine "this leaf just finished for real" signal) is
    /// the intended caller, via a new, optional `Arc<IntentEngine>` it can wire in. Real conflict
    /// detection itself (comparing genuinely `Executing` leaves across active graphs) remains a
    /// separate, larger piece this alone doesn't build -- this closes only the write-back half:
    /// a real caller can now record what actually happened, which is the piece that was missing
    /// before any conflict-detection logic could have anything real to compare.
    pub fn mark_status(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: NodeId,
        status: IntentStatus,
    ) -> Result<(), IntentError> {
        let mut intent = self.get_intent(monitor, token, id)?;
        intent.status = status;
        intent.updated_at = now();
        self.put_intent(monitor, token, Some(id), &intent)?;
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
