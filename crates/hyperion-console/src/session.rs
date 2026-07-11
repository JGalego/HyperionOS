//! The real per-turn pipeline: a typed utterance drives `hyperion-intent`'s real Intent Engine,
//! then either `hyperion-coordination`'s real multi-task allocator (when the utterance matched a
//! real HTN decomposition) or a direct `hyperion-agent-runtime` invocation (when it didn't --
//! see [`ConsoleSession::handle_utterance`]'s docs for why that fallback exists and is still a
//! real Agent invocation, not a shortcut around one), and finally `hyperion-workspace`'s real
//! compiler + accessibility-tree projection, which is what actually turns the outcome into the
//! lines of text this crate's whole job is to produce.
//!
//! Kept separate from `main.rs` so the full real pipeline is testable directly (feed a string in,
//! assert on the real `Vec<String>` that comes back) without needing a real stdin/stdout.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hyperion_agent_runtime::{AgentRuntime, InvokeOutcome};
use hyperion_ai_runtime::{
    sign, LocalAiRuntime, MockBackend, ModelClass, ModelDescriptor, Precision, QuantizedVariant,
};
use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_context::{Budget, ContextBundle, ExpertiseEstimate, ExpertiseLevel, Scope};
use hyperion_coordination::CoordinationSession;
use hyperion_crypto::Keystore;
use hyperion_intent::{HandleOutcome, IntentEngine};
use hyperion_knowledge_graph::{GraphError, KnowledgeGraph, NodeId};
use hyperion_netstack::{DomainEgressGrant, NetstackHub};
use hyperion_workspace::{
    project, CapabilityUiContract, ComplexityTier, Modality, ModalityInterface, RegionAffinity,
    WorkspaceCompiler,
};

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs()
}

/// One real outcome (an HTN task, or the single undecomposed goal) about to be rendered as one
/// real Workspace panel.
struct TaskOutcome {
    predicate: String,
    detail: String,
}

pub struct ConsoleSession {
    monitor: CapabilityMonitor,
    token: CapabilityToken,
    context: Arc<hyperion_context::ContextEngine>,
    intent_engine: IntentEngine,
    coordination: CoordinationSession,
    /// Held separately from `coordination` (which owns its own clone of the same `Arc`):
    /// `CoordinationSession` doesn't expose the runtime it was built with, and the undecomposed-
    /// goal path needs a real `AgentRuntime` handle of its own to spawn/invoke directly.
    agent_runtime: Arc<AgentRuntime>,
    workspace: WorkspaceCompiler,
    next_turn_id: u64,
}

impl ConsoleSession {
    /// `data_dir` is where the real, WAL-backed Knowledge Graph this session's Intent Engine
    /// grounds against lives -- on the real booted image, M6's own dedicated persistent
    /// partition; in a test, any tempdir.
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self, GraphError> {
        let kg_path = PathBuf::from(data_dir.as_ref()).join("console_knowledge_graph.jsonl");
        let mut monitor = CapabilityMonitor::new();
        let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
        let graph = Arc::new(KnowledgeGraph::open(&kg_path)?);
        let context = Arc::new(hyperion_context::ContextEngine::new(graph.clone()));
        let netstack = Arc::new(Self::build_netstack(graph.clone()));
        let intent_engine = IntentEngine::new(graph, context.clone());
        let ai_runtime = Arc::new(Self::build_ai_runtime(data_dir.as_ref()));
        let agent_runtime = Arc::new(AgentRuntime::new_with_netstack(
            ai_runtime,
            Some(netstack.clone()),
        ));
        let coordination = CoordinationSession::new(agent_runtime.clone());

        // A real, permissive domain-egress grant for this session's own root token, minted once
        // here rather than per-call: a real interactive assistant can't pre-enumerate every real
        // domain a user might ask about, so this uses the real "*" wildcard pattern
        // (PRODUCTION_BOOT_PROMPT.md M10 -- see `hyperion_netstack::hub`'s own `domain_matches`
        // doc comment for what this does and doesn't loosen: SSRF containment and the grant's
        // own rate limit still apply regardless of which domain pattern matched).
        let _ = netstack.grant_domain_egress(
            &monitor,
            &token,
            &token,
            DomainEgressGrant {
                domain_patterns: vec!["*".to_string()],
                rate_limit_per_window: 100,
                window_secs: 60,
                max_depth: 1,
                expiry: None,
            },
            now(),
        );

        Ok(ConsoleSession {
            monitor,
            token,
            context,
            intent_engine,
            coordination,
            agent_runtime,
            workspace: WorkspaceCompiler::new(),
            next_turn_id: 1,
        })
    }

    /// Real model selection for this session's own `assistant.respond` calls (see
    /// [`Self::run_undecomposed_goal`]) -- a real, small, CPU-only [`hyperion_ai_runtime::CandleBackend`]
    /// when this binary is built with `--features candle`, [`MockBackend`] otherwise, the exact
    /// "swap the backend, not the call site" principle `hyperion-ai-runtime`'s own doc comment
    /// already names. Off by default for the same reason that feature is off by default in
    /// `hyperion-ai-runtime` itself: a real release image opts in explicitly; every host-side
    /// dev/test build of this console stays fast and network-free.
    ///
    /// A real gap this does *not* solve, named rather than silently assumed: a `candle` build's
    /// first `CandleBackend::load()` call really does hit the network (Hugging Face Hub) unless
    /// `hf-hub`'s on-disk cache is already populated -- fine for a dev loop, not for a real boot
    /// with no network yet up. A real release image needs the model file pre-baked into its
    /// rootfs at build time (a Buildroot post-build step populating that same cache directory),
    /// which is separate work from this milestone. If a `candle` build's real load fails for any
    /// reason (including that network gap), this falls back to [`MockBackend`] rather than
    /// panicking the whole console -- degrading, never crashing on a missing model, exactly the
    /// posture docs/02 §4 invariant 5 already asks this system to take everywhere else.
    ///
    /// PRODUCTION_BOOT_PROMPT.md M9: the registered descriptor is really Ed25519-signed, not a
    /// checksum stand-in -- by this session's own real device identity, a [`Keystore`] persisted
    /// under `data_dir` (the same real, dedicated partition M6 already gives the Knowledge Graph),
    /// so it's stable across reboots rather than a fresh, unverifiable identity every restart.
    fn build_ai_runtime(data_dir: &Path) -> LocalAiRuntime {
        #[cfg(feature = "candle")]
        let backend: Box<dyn hyperion_ai_runtime::InferenceBackend> =
            match hyperion_ai_runtime::CandleBackend::load() {
                Ok(backend) => Box::new(backend),
                Err(e) => {
                    eprintln!(
                        "warning: real Candle model load failed ({e}); falling back to the \
                         mock inference backend for this session"
                    );
                    Box::new(MockBackend)
                }
            };
        #[cfg(not(feature = "candle"))]
        let backend: Box<dyn hyperion_ai_runtime::InferenceBackend> = Box::new(MockBackend);

        let runtime = LocalAiRuntime::new(backend, 8_000);
        let keystore = Keystore::open_or_create(&data_dir.join("device.key"))
            .expect("open or create this session's real device signing key");
        let mut descriptor = ModelDescriptor {
            model_id: 1,
            class: ModelClass::Slm,
            variants: vec![QuantizedVariant {
                precision: Precision::Fp16,
                // Generously above stories15M.bin's real ~61 MB on disk so real-model
                // residency/fit logic never has a reason to reject it (matches
                // hyperion-ai-runtime's own candle_inference.rs test's same choice).
                footprint_mb: 100,
                expected_tokens_per_sec: 10.0,
            }],
            signature: None,
        };
        descriptor.signature = Some(sign(&descriptor, &keystore));
        runtime
            .register_model(descriptor, &keystore.verifying_key())
            .expect("a descriptor this session just really signed always verifies");
        runtime
    }

    /// Real network selection for this session's own `web.research` calls (see
    /// [`Self::run_web_research`]) -- a real [`hyperion_netstack::ReqwestFetchBackend`] +
    /// [`hyperion_netstack::HtmlHeuristicExtractionBackend`] when this binary is built with
    /// `--features real-http`, [`hyperion_netstack::MockFetchBackend`]/
    /// [`hyperion_netstack::MockExtractionBackend`] otherwise -- the exact same "swap the
    /// backend, not the call site" principle [`Self::build_ai_runtime`] already established for
    /// M8 (PRODUCTION_BOOT_PROMPT.md M10). Off by default for the same reason: a real release
    /// image opts in explicitly; every host-side dev/test build of this console stays fast and
    /// network-free. Falls back to the mock backends (degrading, never panicking the whole
    /// console) if a `real-http` build's real client init fails for any reason.
    fn build_netstack(graph: Arc<KnowledgeGraph>) -> NetstackHub {
        #[cfg(feature = "real-http")]
        let (fetch_backend, extraction_backend): (
            Box<dyn hyperion_netstack::FetchBackend>,
            Box<dyn hyperion_netstack::ExtractionBackend>,
        ) = match hyperion_netstack::ReqwestFetchBackend::new() {
            Ok(backend) => (
                Box::new(backend),
                Box::new(hyperion_netstack::HtmlHeuristicExtractionBackend),
            ),
            Err(e) => {
                eprintln!(
                    "warning: real HTTP client init failed ({e}); falling back to the mock \
                     network backend for this session"
                );
                (
                    Box::new(hyperion_netstack::MockFetchBackend::new()),
                    Box::new(hyperion_netstack::MockExtractionBackend),
                )
            }
        };
        #[cfg(not(feature = "real-http"))]
        let (fetch_backend, extraction_backend): (
            Box<dyn hyperion_netstack::FetchBackend>,
            Box<dyn hyperion_netstack::ExtractionBackend>,
        ) = (
            Box::new(hyperion_netstack::MockFetchBackend::new()),
            Box::new(hyperion_netstack::MockExtractionBackend),
        );

        NetstackHub::new(graph, fetch_backend, extraction_backend)
    }

    /// Real utterance in, real rendered text lines out -- M7 stage 1's exit criterion, this
    /// function *is* the pipeline it names: "a real utterance... produces a real Intent Graph, a
    /// real Agent invocation, and real text output." Never returns an `Err`: any real failure
    /// along the way becomes a plain-language line in the returned text (CLAUDE.md's "never
    /// expose technical errors directly" -- this is the boundary where that applies), not a
    /// panic or a propagated error a caller must handle.
    pub fn handle_utterance(&mut self, utterance: &str) -> Vec<String> {
        let turn_tag = format!("console-turn-{}", self.next_turn_id);
        self.next_turn_id += 1;

        let outcome = match self.intent_engine.handle_utterance(
            &self.monitor,
            &self.token,
            utterance,
            &turn_tag,
        ) {
            Ok(outcome) => outcome,
            Err(e) => return vec![format!("I couldn't understand that: {e}")],
        };

        let root = match outcome {
            HandleOutcome::Submitted(root) => root,
            HandleOutcome::NeedsClarification {
                mention,
                candidates,
            } => {
                return vec![format!(
                    "I'm not sure which \"{mention}\" you mean ({} possibilities) -- could you \
                     be more specific?",
                    candidates.len()
                )];
            }
        };

        let ticket = match self.intent_engine.submit(&self.monitor, &self.token, root) {
            Ok(ticket) => ticket,
            Err(e) => return vec![format!("I understood that, but couldn't act on it: {e}")],
        };

        let (predicate, outcomes) = if ticket.ready_leaves.is_empty() {
            self.run_undecomposed_goal(root, utterance)
        } else {
            self.run_decomposed_plan(&ticket)
        };

        self.render_workspace(root, &turn_tag, &predicate, &outcomes)
    }

    /// The one built-in HTN template ("launch my startup") is the only utterance shape that
    /// decomposes into real dependent sub-tasks today; everything else becomes a single,
    /// undecomposed root Intent with no children -- `hyperion-coordination::create_session`
    /// builds its task list from a root's *children* alone (see its own source), so a plan with
    /// none would have nothing to ever allocate. Rather than silently do nothing for the common
    /// case, this still drives one real Agent invocation directly against the root goal itself,
    /// via the same real `AgentRuntime::spawn`/`invoke` mechanism `hyperion-coordination` uses
    /// internally -- a real Agent invocation regardless of which path a given utterance takes.
    ///
    /// PRODUCTION_BOOT_PROMPT.md M8: this default action is the "assistant" specialization's
    /// real `assistant.respond` capability -- a real generated response from this session's own
    /// [`Self::build_ai_runtime`], not `web.search`'s canned stub string. This is *not*
    /// `hyperion-intent`'s own deferred "generative decomposition" (docs/05 §2's fallback that
    /// would produce a real multi-leaf plan from a model) -- the root Intent here still has no
    /// children, same as before; only the one leaf action taken *about* it is now really
    /// model-driven instead of a stub. Producing a genuine structured decomposition from a goal
    /// shape with no template remains exactly as deferred as `hyperion-intent`'s own doc comment
    /// already says, and deliberately so: the tiny (real, but non-instruction-tuned) model this
    /// session runs by default cannot reliably follow a "decompose this into steps" instruction,
    /// and fabricating leaf tasks from output it can't actually produce reliably would be the
    /// "pretend" that crate's own doc comment already rules out.
    ///
    /// PRODUCTION_BOOT_PROMPT.md M10: an utterance containing something that looks like a real
    /// URL routes to [`Self::run_web_research`] instead -- a real, minimal, deterministic
    /// utterance-shape check (not a general NLU pipeline), matching the same
    /// deterministic-keyword-matching convention `hyperion-intent`'s own `URGENCY_KEYWORDS`/
    /// `CANCEL_KEYWORDS` already use.
    fn run_undecomposed_goal(
        &mut self,
        root: NodeId,
        utterance: &str,
    ) -> (String, Vec<TaskOutcome>) {
        if let Some(url) = extract_url(utterance) {
            return self.run_web_research(root, url);
        }

        let manifest = hyperion_coordination::default_manifests()
            .into_iter()
            .find(|m| m.specialization == "assistant")
            .expect("default_manifests always includes the assistant specialization");

        let detail =
            match self
                .agent_runtime
                .spawn(&self.monitor, &self.token, manifest, Some(root.0))
            {
                Ok(instance_id) => {
                    let args = serde_json::json!({ "prompt": utterance });
                    match self.agent_runtime.invoke(
                        &self.monitor,
                        &self.token,
                        instance_id,
                        "assistant.respond",
                        args,
                    ) {
                        Ok(InvokeOutcome::Result(value)) => {
                            let text = value.get("text").and_then(|v| v.as_str()).unwrap_or("");
                            format!("done -- {text}")
                        }
                        Ok(InvokeOutcome::Denied) => "denied".to_string(),
                        Ok(InvokeOutcome::PendingConsent) => "needs your consent first".to_string(),
                        Ok(InvokeOutcome::QuotaExceeded) => "over quota right now".to_string(),
                        Ok(InvokeOutcome::Failed(reason)) => format!("failed -- {reason}"),
                        Err(e) => format!("failed -- {e}"),
                    }
                }
                Err(e) => format!("failed -- couldn't start an Agent: {e}"),
            };

        (
            "generic_goal".to_string(),
            vec![TaskOutcome {
                predicate: "generic_goal".to_string(),
                detail,
            }],
        )
    }

    /// PRODUCTION_BOOT_PROMPT.md M10's real deliverable: a URL-shaped undecomposed goal drives
    /// the "research" specialization's real `web.research` capability -- a real fetch over the
    /// real network, a real (non-model) HTML extraction, and a real merge into this session's
    /// own real Knowledge Graph via [`Self::build_netstack`], not a stub. Reuses the same real
    /// `AgentRuntime::spawn`/`invoke` mechanism every other action in this pipeline goes through,
    /// gated by the exact same Broker/quota/circuit-breaker checks.
    fn run_web_research(&mut self, root: NodeId, url: &str) -> (String, Vec<TaskOutcome>) {
        let manifest = hyperion_coordination::default_manifests()
            .into_iter()
            .find(|m| m.specialization == "research")
            .expect("default_manifests always includes the research specialization");

        let detail =
            match self
                .agent_runtime
                .spawn(&self.monitor, &self.token, manifest, Some(root.0))
            {
                Ok(instance_id) => {
                    let args = serde_json::json!({ "url": url });
                    match self.agent_runtime.invoke(
                        &self.monitor,
                        &self.token,
                        instance_id,
                        "web.research",
                        args,
                    ) {
                        Ok(InvokeOutcome::Result(value)) => {
                            let needs_review = value
                                .get("needs_review")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            if needs_review {
                                "done -- merged, but flagged for your review (an ambiguous match)"
                                    .to_string()
                            } else {
                                "done -- merged into the knowledge graph".to_string()
                            }
                        }
                        Ok(InvokeOutcome::Denied) => "denied".to_string(),
                        Ok(InvokeOutcome::PendingConsent) => "needs your consent first".to_string(),
                        Ok(InvokeOutcome::QuotaExceeded) => "over quota right now".to_string(),
                        Ok(InvokeOutcome::Failed(reason)) => format!("failed -- {reason}"),
                        Err(e) => format!("failed -- {e}"),
                    }
                }
                Err(e) => format!("failed -- couldn't start an Agent: {e}"),
            };

        (
            "generic_goal".to_string(),
            vec![TaskOutcome {
                predicate: "generic_goal".to_string(),
                detail,
            }],
        )
    }

    /// The real, multi-task path: `hyperion-coordination`'s own dependency-respecting allocator,
    /// exactly `hyperion-coordination/tests/worked_trace.rs`'s own wiring (this crate's Cargo.toml
    /// has no `[dev-dependencies]` on that crate; the sequence is reproduced here, not imported,
    /// since it's the calling *pattern* this crate needs, not a helper function that crate
    /// exports).
    fn run_decomposed_plan(
        &mut self,
        ticket: &hyperion_intent::ExecutionTicket,
    ) -> (String, Vec<TaskOutcome>) {
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

        // Drive every dependency wave to completion -- each `allocate` call is one real tick;
        // an empty result means every task is Done, Failed, or Blocked with nothing left ready.
        loop {
            match self
                .coordination
                .allocate(&self.monitor, &self.token, session_id)
            {
                Ok(records) if records.is_empty() => break,
                Ok(_) => continue,
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
                    detail: format!("{:?}", node.status),
                })
                .collect(),
            Err(e) => vec![TaskOutcome {
                predicate: "plan".to_string(),
                detail: format!("plan completed but couldn't be read back: {e}"),
            }],
        };

        ("plan".to_string(), outcomes)
    }

    /// The literal M7 deliverable: "drive `hyperion-workspace`'s compiled UI/accessibility trees
    /// through a real TTY renderer." One real panel per outcome, compiled for real, then
    /// projected through the real `Modality::ScreenReader` linearization -- the same accessible
    /// text a screen reader would announce, which is exactly what "real text output rendered to
    /// the real TTY" means for a text-first console (docs/14 §2 frames a text/voice-first
    /// interface as accessibility's primary case, not a fallback).
    fn render_workspace(
        &self,
        root: NodeId,
        turn_tag: &str,
        predicate: &str,
        outcomes: &[TaskOutcome],
    ) -> Vec<String> {
        let contracts: Vec<CapabilityUiContract> = outcomes
            .iter()
            .map(|o| contract_for(&o.predicate, &format!("{}: {}", o.predicate, o.detail)))
            .collect();

        // A real Context Bundle assembly for this turn's own scope -- falls back to an empty
        // bundle (still a real, valid `ContextBundle`, just with nothing to bind panels to yet)
        // if assembly itself fails, since a missing context signal shouldn't block rendering the
        // real Agent outcome the user is actually waiting on.
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
            Err(e) => return vec![format!("(couldn't render a workspace for this: {e})")],
        };
        let _ = self
            .workspace
            .mount(&self.monitor, &self.token, graph.graph_id);

        let template = self
            .workspace
            .get_template(predicate, &contracts, ComplexityTier::Beginner)
            .expect("the template just compiled above is always cached under this same key");

        match project(&template.accessibility_tree, Modality::ScreenReader) {
            ModalityInterface::ScreenReader(lines) => lines,
            other => unreachable!(
                "project(_, Modality::ScreenReader) always returns ScreenReader, got {other:?}"
            ),
        }
    }
}

/// A minimal, real, deterministic utterance-shape recognizer (PRODUCTION_BOOT_PROMPT.md M10) --
/// not a general NLU pipeline, just "does this utterance contain something that looks like a
/// URL," matching the same deterministic-keyword-matching convention `hyperion-intent`'s own
/// `URGENCY_KEYWORDS`/`CANCEL_KEYWORDS` already use. A trailing punctuation mark immediately
/// after the URL (e.g. a sentence-ending period) is not stripped -- a real, named imperfection,
/// not silently assumed correct; precise control just means not punctuating immediately after
/// a URL.
fn extract_url(utterance: &str) -> Option<&str> {
    utterance
        .split_whitespace()
        .find(|word| word.starts_with("http://") || word.starts_with("https://"))
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
