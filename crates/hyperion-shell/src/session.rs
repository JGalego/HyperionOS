//! [`EmbeddedSession`]: this crate's own real, trimmed turn pipeline -- see lib.rs's doc comment
//! on why this is a sibling of `hyperion_console::ConsoleSession`'s pipeline rather than a shared
//! dependency on it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use hyperion_agent_runtime::{AgentRuntime, InvokeOutcome};
use hyperion_ai_runtime::{sign, LocalAiRuntime, MockBackend, ModelClass, ModelDescriptor, Precision, QuantizedVariant};
use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_context::{Budget, ContextBundle, ContextEngine, ExpertiseEstimate, ExpertiseLevel, Scope};
use hyperion_coordination::{CoordinationSession, TaskStatus};
use hyperion_crypto::Keystore;
use hyperion_intent::{HandleOutcome, IntentEngine};
use hyperion_knowledge_graph::{GraphError, KnowledgeGraph, NodeId};
use hyperion_workspace::{
    project, CapabilityUiContract, ComplexityTier, Modality, ModalityInterface, RegionAffinity,
    WorkspaceCompiler,
};

use crate::{IntentSink, TurnOutcome};

/// One real outcome (an HTN task, or the single undecomposed goal) about to become one real
/// Workspace panel -- same shape `hyperion_console::ConsoleSession`'s own private `TaskOutcome`
/// uses, reproduced here rather than shared for the same reason lib.rs names.
struct TaskOutcome {
    predicate: String,
    detail: String,
}

/// This crate's own real turn pipeline: Intent Engine -> Coordination (a decomposed plan) or a
/// direct Agent invoke (an undecomposed goal) -> `WorkspaceCompiler`. Always runs
/// `hyperion-ai-runtime`'s deterministic mock backend -- see lib.rs's deferred-scope list for
/// why real backend selection isn't re-derived here.
pub struct EmbeddedSession {
    monitor: CapabilityMonitor,
    token: CapabilityToken,
    context: Arc<ContextEngine>,
    intent_engine: IntentEngine,
    coordination: CoordinationSession,
    agent_runtime: Arc<AgentRuntime>,
    assistant_instance_id: u64,
    workspace: WorkspaceCompiler,
    next_turn_id: u64,
}

impl EmbeddedSession {
    /// `data_dir` is where this session's own real, WAL-backed Knowledge Graph and device
    /// signing key live -- any writable directory; a tempdir in tests.
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self, GraphError> {
        let data_dir = data_dir.as_ref();
        let kg_path = data_dir.join("shell_knowledge_graph.jsonl");
        let mut monitor = CapabilityMonitor::new();
        let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
        let graph = Arc::new(KnowledgeGraph::open(&kg_path)?);
        let context = Arc::new(ContextEngine::new(graph.clone()));
        let intent_engine = IntentEngine::new(graph.clone(), context.clone());

        let keystore = Keystore::open_or_create(&data_dir.join("device.key"))
            .expect("open or create this session's own real device signing key");
        let ai_runtime = Arc::new(Self::build_ai_runtime(&keystore));
        let agent_runtime = Arc::new(AgentRuntime::new(ai_runtime));
        let coordination = CoordinationSession::new(agent_runtime.clone(), graph);

        let assistant_manifest = hyperion_coordination::default_manifests()
            .into_iter()
            .find(|m| m.specialization == "assistant")
            .expect("default_manifests always includes the assistant specialization");
        let assistant_instance_id = agent_runtime
            .spawn(&monitor, &token, assistant_manifest, None)
            .expect("spawn this session's own persistent assistant Agent instance");

        Ok(EmbeddedSession {
            monitor,
            token,
            context,
            intent_engine,
            coordination,
            agent_runtime,
            assistant_instance_id,
            workspace: WorkspaceCompiler::new(),
            next_turn_id: 1,
        })
    }

    /// Registers a real, signed model descriptor over `hyperion-ai-runtime`'s deterministic
    /// [`MockBackend`] -- matches `hyperion_console::ConsoleSession`'s own real model-descriptor
    /// signing/registration, minus the `candle`-feature-gated real-inference branch that crate
    /// carries (deferred here; see lib.rs).
    fn build_ai_runtime(keystore: &Keystore) -> LocalAiRuntime {
        let runtime = LocalAiRuntime::new(Box::new(MockBackend), 8_000);
        let mut descriptor = ModelDescriptor {
            model_id: 1,
            class: ModelClass::Slm,
            variants: vec![QuantizedVariant {
                precision: Precision::Fp16,
                footprint_mb: 100,
                expected_tokens_per_sec: 10.0,
            }],
            signature: None,
        };
        descriptor.signature = Some(sign(&descriptor, keystore));
        runtime
            .register_model(descriptor, &keystore.verifying_key())
            .expect("a descriptor this session just really signed always verifies");
        runtime
    }

    fn run_undecomposed_goal(&mut self, utterance: &str) -> (String, Vec<TaskOutcome>) {
        let args = serde_json::json!({ "prompt": utterance });
        let detail = match self.agent_runtime.invoke(
            &self.monitor,
            &self.token,
            self.assistant_instance_id,
            "assistant.respond",
            args,
        ) {
            Ok(InvokeOutcome::Result(value)) => {
                let text = value.get("text").and_then(|v| v.as_str()).unwrap_or("");
                format!("done -- {text}")
            }
            Ok(InvokeOutcome::Denied) => "denied".to_string(),
            // `assistant.respond` is a baseline capability under the mock backend this crate
            // always runs -- never actually gated, so this arm is unreachable in practice today,
            // named honestly rather than silently assumed away.
            Ok(InvokeOutcome::PendingConsent) => {
                "needs consent -- this shell doesn't have a consent flow yet".to_string()
            }
            Ok(InvokeOutcome::QuotaExceeded) => "over quota right now".to_string(),
            Ok(InvokeOutcome::Failed(reason)) => format!("failed -- {reason}"),
            Err(e) => format!("failed -- {e}"),
        };

        (
            "generic_goal".to_string(),
            vec![TaskOutcome {
                predicate: "generic_goal".to_string(),
                detail,
            }],
        )
    }

    /// The real, multi-task path -- `hyperion-coordination`'s own dependency-respecting
    /// allocator, ticked to completion. Unlike `hyperion_console::ConsoleSession`'s own
    /// `run_decomposed_plan`, there is no per-tick progress callback here (see lib.rs's
    /// deferred-scope list): this call blocks the background thread [`crate::ShellApp`] spawned
    /// for it until the whole plan converges, then returns once.
    fn run_decomposed_plan(&mut self, ticket: &hyperion_intent::ExecutionTicket) -> (String, Vec<TaskOutcome>) {
        let session_id = match self.coordination.create_session(
            &self.monitor,
            &self.token,
            &self.intent_engine,
            ticket,
        ) {
            Ok(id) => id,
            Err(e) => {
                return (
                    "plan".to_string(),
                    vec![TaskOutcome {
                        predicate: "plan".to_string(),
                        detail: format!("failed to start coordinating this plan: {e}"),
                    }],
                )
            }
        };

        loop {
            match self
                .coordination
                .allocate(&self.monitor, &self.token, session_id)
            {
                Ok(records) if records.is_empty() => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }

        let outcomes = match self
            .coordination
            .get_plan(&self.monitor, &self.token, session_id)
        {
            Ok(plan) => plan
                .nodes
                .iter()
                .map(|node| TaskOutcome {
                    predicate: node.description.clone(),
                    detail: render_task_detail(node.status, node.result.as_ref()),
                })
                .collect(),
            Err(e) => vec![TaskOutcome {
                predicate: "plan".to_string(),
                detail: format!("plan completed but couldn't be read back: {e}"),
            }],
        };

        ("plan".to_string(), outcomes)
    }

    /// The literal deliverable: drive `hyperion-workspace`'s real compiler + accessibility-tree
    /// derivation over this turn's outcomes, exactly `hyperion_console::ConsoleSession::
    /// render_workspace`'s own pipeline -- projected as `Modality::ScreenReader` narration here
    /// too (kept for a history pane and for parity with the console's own output), plus the real
    /// compiled graph/tree [`crate::ShellApp`] actually paints.
    fn render_workspace(
        &self,
        root: NodeId,
        turn_tag: &str,
        predicate: &str,
        outcomes: &[TaskOutcome],
    ) -> TurnOutcome {
        let contracts: Vec<CapabilityUiContract> = outcomes
            .iter()
            .map(|o| contract_for(&o.predicate, &format!("{}: {}", o.predicate, o.detail)))
            .collect();

        let scope = Scope {
            intent_id: root.0.to_string(),
            session_id: turn_tag.to_string(),
            mentions: Vec::new(),
            anchors: Vec::new(),
        };
        let bundle = self
            .context
            .assemble(&self.monitor, &self.token, &scope, Budget::default())
            .unwrap_or_else(|_| empty_context_bundle(scope));

        let graph = match self.workspace.compile(
            &self.monitor,
            &self.token,
            root,
            predicate,
            &contracts,
            &bundle,
            ComplexityTier::Beginner,
            1.0,
        ) {
            Ok(graph) => graph,
            Err(e) => {
                return TurnOutcome {
                    graph: None,
                    tree: None,
                    narration: vec![format!("(couldn't render a workspace for this: {e})")],
                }
            }
        };
        let _ = self
            .workspace
            .mount(&self.monitor, &self.token, graph.graph_id);

        let template = self
            .workspace
            .get_template(predicate, &contracts, ComplexityTier::Beginner)
            .expect("the template just compiled above is always cached under this same key");

        let narration = match project(&template.accessibility_tree, Modality::ScreenReader) {
            ModalityInterface::ScreenReader(lines) => lines,
            other => unreachable!(
                "project(_, Modality::ScreenReader) always returns ScreenReader, got {other:?}"
            ),
        };

        TurnOutcome {
            graph: Some(graph),
            tree: Some(template.accessibility_tree),
            narration,
        }
    }
}

impl IntentSink for EmbeddedSession {
    fn handle_utterance(&mut self, utterance: &str) -> TurnOutcome {
        let turn_tag = format!("shell-turn-{}", self.next_turn_id);
        self.next_turn_id += 1;

        let outcome = match self.intent_engine.handle_utterance(
            &self.monitor,
            &self.token,
            utterance,
            &turn_tag,
        ) {
            Ok(outcome) => outcome,
            Err(e) => {
                return TurnOutcome {
                    graph: None,
                    tree: None,
                    narration: vec![format!("I couldn't understand that: {e}")],
                }
            }
        };

        let root = match outcome {
            HandleOutcome::Submitted(root) => root,
            HandleOutcome::NeedsClarification {
                mention,
                candidates,
            } => {
                return TurnOutcome {
                    graph: None,
                    tree: None,
                    narration: vec![format!(
                        "I'm not sure which \"{mention}\" you mean ({} possibilities) -- could \
                         you be more specific?",
                        candidates.len()
                    )],
                }
            }
        };

        let ticket = match self.intent_engine.submit(&self.monitor, &self.token, root) {
            Ok(ticket) => ticket,
            Err(e) => {
                return TurnOutcome {
                    graph: None,
                    tree: None,
                    narration: vec![format!(
                        "I understood that, but couldn't act on it: {e}"
                    )],
                }
            }
        };

        let (predicate, outcomes) = if ticket.ready_leaves.is_empty() {
            self.run_undecomposed_goal(utterance)
        } else {
            self.run_decomposed_plan(&ticket)
        };

        self.render_workspace(root, &turn_tag, &predicate, &outcomes)
    }
}

/// A short, honest preview of a completed task's real result -- v1's own minimal stand-in for
/// `hyperion_console::graph_explorer::render_capability_result`/`preview` (both private to that
/// crate): tries the `"text"` field every mock-backend dispatch actually returns, falling back to
/// the raw JSON for any other shape, truncated to one line/100 chars so a plan with several
/// completed tasks doesn't flood one panel.
fn render_task_detail(status: TaskStatus, result: Option<&serde_json::Value>) -> String {
    match result.and_then(|v| v.get("text")).and_then(|v| v.as_str()) {
        Some(text) => format!("{status:?} -- {}", truncate(text)),
        None => format!("{status:?}"),
    }
}

fn truncate(text: &str) -> String {
    let first_line = text.lines().next().unwrap_or("");
    if first_line.chars().count() > 100 {
        let head: String = first_line.chars().take(100).collect();
        format!("{head}...")
    } else {
        first_line.to_string()
    }
}

fn contract_for(capability_ref: &str, label: &str) -> CapabilityUiContract {
    CapabilityUiContract {
        capability_ref: capability_ref.to_string(),
        panel_template: format!("{capability_ref}.default"),
        region_affinity: RegionAffinity::Center,
        min_size: (200, 200),
        priority: 0.5,
        binds_category: None,
        variants: HashMap::new(),
        accessible_role: Some("status".to_string()),
        label_template: Some(label.to_string()),
        keyboard_operations: vec!["activate".to_string()],
        alt_text_hook: None,
        contrast_ratio: 7.0,
        has_motion: false,
        reduced_motion_alternative: true,
        language_tag: "en".to_string(),
        emits_audio: false,
        has_visual_alert_equivalent: true,
    }
}

fn empty_context_bundle(scope: Scope) -> ContextBundle {
    ContextBundle {
        bundle_id: 0,
        scope,
        entries: Vec::new(),
        assembled_at: 0,
        budget: Budget::default(),
        expertise_signal: ExpertiseEstimate {
            domain: "general".to_string(),
            level: ExpertiseLevel::Novice,
            evidence: Vec::new(),
            confidence: 0.0,
        },
    }
}
