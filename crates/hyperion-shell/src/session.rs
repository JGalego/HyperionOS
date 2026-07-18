//! [`EmbeddedSession`]: this crate's own real turn pipeline, now really shared with
//! `hyperion_console::ConsoleSession`'s pipeline via `hyperion-turn` -- see that crate's own doc
//! comment for exactly what's shared and why (and what, correctly, isn't).

use std::path::Path;
use std::sync::Arc;

use hyperion_agent_runtime::{AgentRuntime, InvokeOutcome};
use hyperion_ai_runtime::{
    sign, LocalAiRuntime, MockBackend, ModelClass, ModelDescriptor, Precision, QuantizedVariant,
};
use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_coordination::{CoordinationSession, TaskNode};
use hyperion_crypto::Keystore;
use hyperion_intent::IntentEngine;
use hyperion_knowledge_graph::{GraphError, KnowledgeGraph};
use hyperion_turn::{contract_for, render_workspace, start_turn, TaskOutcome, TurnStart};
use hyperion_workspace::WorkspaceCompiler;

use crate::{IntentSink, TurnOutcome};

/// This crate's own real turn pipeline: Intent Engine -> Coordination (a decomposed plan) or a
/// direct Agent invoke (an undecomposed goal) -> `WorkspaceCompiler`, via `hyperion-turn`'s shared
/// implementation of that pipeline's own mechanical middle -- see this module's own doc comment.
/// Always runs `hyperion-ai-runtime`'s deterministic mock backend -- see lib.rs's deferred-scope
/// list for why real backend selection isn't re-derived here.
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

    /// A short, honest preview of a completed task's real result -- v1's own minimal stand-in for
    /// `hyperion_console::graph_explorer::render_capability_result`/`preview` (both private to
    /// that crate, and deliberately not shared -- see `hyperion-turn`'s own doc comment): tries
    /// the `"text"` field every mock-backend dispatch actually returns, falling back to the raw
    /// JSON for any other shape, truncated to one line/100 chars so a plan with several completed
    /// tasks doesn't flood one panel.
    fn render_task_detail(node: &TaskNode) -> String {
        match node
            .result
            .as_ref()
            .and_then(|v| v.get("text"))
            .and_then(|v| v.as_str())
        {
            Some(text) => format!("{:?} -- {}", node.status, truncate(text)),
            None => format!("{:?}", node.status),
        }
    }
}

impl IntentSink for EmbeddedSession {
    fn handle_utterance(&mut self, utterance: &str) -> TurnOutcome {
        let turn_tag = format!("shell-turn-{}", self.next_turn_id);
        self.next_turn_id += 1;

        let (root, ticket) = match start_turn(
            &self.monitor,
            &self.token,
            &self.intent_engine,
            &self.context,
            utterance,
            &turn_tag,
        ) {
            TurnStart::Ready { root, ticket } => (root, ticket),
            // This crate mints a fresh, unique `session_id` per turn (`turn_tag`, above), so
            // `hyperion-intent`'s think mode -- opt-in *per session* -- has no persistent session
            // here to ever be toggled on for; this arm exists only so the match stays exhaustive,
            // not because this crate's own pipeline can actually produce it today.
            TurnStart::PendingThink { .. } => {
                return TurnOutcome {
                    graph: None,
                    tree: None,
                    narration: vec![
                        "That paused before deciding what it means, but this session has no way \
                         to resume it -- try again without think mode."
                            .to_string(),
                    ],
                }
            }
            TurnStart::Reply(narration) => {
                return TurnOutcome {
                    graph: None,
                    tree: None,
                    narration,
                }
            }
        };

        let (predicate, outcomes) = if ticket.ready_leaves.is_empty() {
            self.run_undecomposed_goal(utterance)
        } else {
            let outcome = hyperion_turn::drive_decomposed_plan(
                &self.monitor,
                &self.token,
                &self.coordination,
                &self.intent_engine,
                &ticket,
                &mut |_| {},
                Self::render_task_detail,
            );
            (outcome.predicate, outcome.outcomes)
        };

        let contracts: Vec<_> = outcomes
            .iter()
            .map(|o| contract_for(&o.predicate, &format!("{}: {}", o.predicate, o.detail)))
            .collect();

        match render_workspace(
            &self.monitor,
            &self.token,
            &self.context,
            &self.workspace,
            root,
            &turn_tag,
            &predicate,
            &contracts,
        ) {
            Ok(rendered) => TurnOutcome {
                graph: Some(rendered.graph),
                tree: Some(rendered.tree),
                narration: rendered.narration,
            },
            Err(msg) => TurnOutcome {
                graph: None,
                tree: None,
                narration: vec![msg],
            },
        }
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
