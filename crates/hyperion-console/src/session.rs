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
use hyperion_coordination::{CoordinationSession, TaskNode};
use hyperion_crypto::{Keystore, SecretStore};
use hyperion_intent::{HandleOutcome, IntentEngine};
use hyperion_knowledge_graph::{GraphError, KnowledgeGraph, NodeId};

use crate::graph_explorer::GraphExplorer;
use hyperion_netstack::{DomainEgressGrant, NetstackHub};
use hyperion_workspace::{
    project, CapabilityUiContract, ComplexityTier, Modality, ModalityInterface, RegionAffinity,
    WorkspaceCompiler,
};

/// `pub(crate)` so [`crate::graph_explorer`]'s own relative-time rendering shares one clock
/// reading with the rest of this crate rather than duplicating this exact function.
pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs()
}

/// A safe, non-revealing preview of a real secret -- just enough (a real prefix/suffix, the same
/// convention every real API-key management UI already uses) for a user to visually confirm what
/// actually got stored, without this crate ever displaying the real secret itself anywhere. Used
/// by [`ConsoleSession::finish_connect`] so a corrupted paste (a stray control character, a
/// terminal artifact) is visible immediately, not discovered later as an opaque real 401.
fn mask_secret(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 8 {
        return "*".repeat(chars.len());
    }
    let prefix: String = chars[..4].iter().collect();
    let suffix: String = chars[chars.len() - 4..].iter().collect();
    format!("{prefix}...{suffix}")
}

/// One real outcome (an HTN task, or the single undecomposed goal) about to be rendered as one
/// real Workspace panel.
struct TaskOutcome {
    predicate: String,
    detail: String,
}

/// One real event [`ConsoleSession::handle_utterance_with_progress`]'s own callback fires while a
/// decomposed multi-task plan works through a tick -- `main.rs` uses `Starting` to know which
/// task names to show as in-flight (e.g. a spinner) and `Done` to know when to stop and print the
/// real result instead.
pub enum TaskProgress {
    /// These tasks are about to be dispatched, concurrently, this tick -- named *before* the
    /// real (potentially slow) capability call blocks this thread, not only after.
    Starting(Vec<String>),
    /// One task's own real, final line (e.g. `"  market_research: Done"`), once its tick's
    /// blocking dispatch has actually returned.
    Done(String),
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
    /// Held so the `/backend`/`use backend` meta-command can swap the live backend without
    /// restarting the process -- `agent_runtime` above only holds its own clone of this same
    /// `Arc`, with no way to reach back out to it.
    ai_runtime: Arc<LocalAiRuntime>,
    current_backend: BackendKind,
    /// Spawned once in [`Self::open`] and reused by every [`Self::run_undecomposed_goal`] turn,
    /// rather than a fresh `AgentInstance` per turn (this crate's own pre-Phase-2 behavior) --
    /// required for ANY capability grant to survive past a single turn at all, cloud consent
    /// included: `resolve_grant`'s per-instance `grants` are empty on every fresh spawn, so a
    /// respawned instance would re-trigger `PendingConsent` forever. Named trade-off: this
    /// instance's own `bound_intent` (scheduler bookkeeping only) is `None` forever now, since
    /// there's no single root `NodeId` at session-open time to bind it to -- cosmetic, not
    /// correctness-affecting for the real admission gate.
    assistant_instance_id: u64,
    /// Real, encrypted-at-rest cloud provider API keys -- see [`hyperion_crypto::SecretStore`].
    /// A stored key proves a real account *exists* (the user only ever gets one in via an
    /// explicit "connect my `<provider>`" utterance) -- deliberately NOT also a standing grant
    /// to *use* it across restarts (see [`Self::open`]'s own doc comment on why): a fresh boot's
    /// first real cloud dispatch still goes through a genuine `InvokeOutcome::PendingConsent`
    /// round trip, once per boot.
    secret_store: SecretStore,
    /// Set while a "connect my `<provider>`" flow is awaiting its follow-up API-key line -- the
    /// *next* call to [`Self::handle_utterance`] is captured as the real secret instead of
    /// parsed as an utterance or meta-command. See [`Self::awaiting_secret_input`], which
    /// `main.rs` checks before each real `read_line` so that one line isn't echoed to the
    /// terminal.
    pending_connect: Option<CloudProvider>,
    /// Set while a live `InvokeOutcome::PendingConsent` is awaiting its yes/no confirmation --
    /// the *next* utterance is captured as that answer rather than parsed normally.
    pending_consent: Option<PendingCloudConsent>,
    /// Backs the `/recall`/`/why`/`/related` meta-commands -- see
    /// [`crate::graph_explorer::GraphExplorer`] for why this is its own small module rather than
    /// more direct `KnowledgeGraph` calls scattered through this file.
    graph_explorer: GraphExplorer,
    workspace: WorkspaceCompiler,
    /// Stable for this `ConsoleSession`'s entire real process lifetime -- the identity
    /// `hyperion-intent`'s working-memory turn buffer / active-graph reconciliation stack and
    /// `hyperion-context`'s working-set hysteresis both accumulate real state against, keyed by
    /// session. A real, previously-shipped bug this fixes: every turn used to mint its own
    /// fresh, unique tag and pass *that* as the session id, so those three real, already-tested
    /// mechanisms never saw more than one turn's worth of history before being silently thrown
    /// away and recreated empty on the very next turn -- a real user asking a follow-up question
    /// in the same conversation got no benefit from any of them.
    session_id: String,
    /// The most recent decomposed plan's own `hyperion-coordination` session id, set at the end
    /// of [`Self::run_decomposed_plan`] -- [`Self::redo_task`] (the `/redo <task> <extra
    /// instructions>` meta-command) needs this to find the real plan a task name belongs to,
    /// since `hyperion-coordination::CoordinationSession::amend_task` is keyed by session, not
    /// by task name alone. `None` until this session's first real decomposed plan runs; `/redo`
    /// gives an honest "nothing to redo yet" reply until then rather than guessing a session.
    last_plan_session_id: Option<u64>,
    /// This session's own real, persistent Ed25519 device identity (docs/998-roadmap.md's
    /// Social pillar: "real cross-instance discovery, identity, and trust" gap, *identity* half)
    /// -- previously created in [`Self::open`] only transiently (to sign a model descriptor and
    /// derive the cloud-secret encryption key) and then dropped, with no way for anything else to
    /// sign or present this session's own identity. Retained here so a real A2A/MCP server can
    /// present a real, verifiable public key and sign its own real replies with it -- see
    /// [`Self::verifying_key`]/[`Self::sign`].
    keystore: Keystore,
    /// docs/998-roadmap.md's Backlog "Protect the Human" item: the most recent real, paused root
    /// Intent from a `HandleOutcome::PendingThink` (this session's think mode is on -- see
    /// [`Self::handle_meta_command`]'s `/think` commands), if any -- what `/think-proceed`
    /// resolves against, since a human wouldn't type a raw `NodeId`.
    pending_think_root: Option<NodeId>,
}

/// The real prompt and capability this session is waiting to re-invoke once a live
/// `InvokeOutcome::PendingConsent` is confirmed -- see [`ConsoleSession::pending_consent`].
struct PendingCloudConsent {
    capability_ref: String,
    prompt: String,
}

/// Which real [`hyperion_ai_runtime::InferenceBackend`] is currently answering
/// `assistant.respond` calls -- tracked here (not in `hyperion-ai-runtime` itself) because
/// "candle"/"mock"/a local engine's own label are console-level, human-facing labels for
/// backends this crate is the one that actually constructs; the runtime crate only knows the
/// trait object, never these names. Not `Copy` (unlike its first two variants alone would allow)
/// because `Engine` carries real, runtime-chosen connection details.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BackendKind {
    /// A real, small, CPU-only Candle model -- see [`hyperion_ai_runtime::CandleBackend`].
    Candle,
    /// The deterministic echo stub -- see [`MockBackend`]. Never a real answer, only ever a
    /// dev/test fallback or explicit choice.
    Mock,
    /// A real OpenAI-compatible local engine or proxy -- see
    /// [`hyperion_ai_runtime::OpenAiCompatBackend`]. `base_url` never has a trailing slash
    /// (normalized in [`ConsoleSession::parse_engine_args`]), so `PartialEq`-based no-op
    /// detection in [`ConsoleSession::switch_backend`] isn't fooled by a trailing-slash-only
    /// difference.
    Engine {
        engine: EngineKind,
        base_url: String,
        model: String,
    },
    /// A real, paid, external cloud provider -- see [`hyperion_ai_runtime::openai_compat_backend`]
    /// (OpenAI itself, reused verbatim -- Groq too, since its API is wire-compatible with
    /// OpenAI's), [`hyperion_ai_runtime::anthropic_backend`], and
    /// [`hyperion_ai_runtime::gemini_backend`]. Unlike `Engine` (self-hosted, never gated), every
    /// dispatch under this variant goes through this provider's own requestable Capability (see
    /// [`Self::capability_ref`]) -- a real consent prompt, not just a runtime switch.
    Cloud {
        provider: CloudProvider,
        model: String,
    },
}

impl BackendKind {
    fn label(&self) -> String {
        match self {
            BackendKind::Candle => "candle".to_string(),
            BackendKind::Mock => "mock".to_string(),
            BackendKind::Engine {
                engine,
                base_url,
                model,
            } => format!("{} (model {model:?} at {base_url})", engine.label()),
            BackendKind::Cloud { provider, model } => {
                format!("{} (model {model:?})", provider.label())
            }
        }
    }

    /// Which real Capability [`ConsoleSession::run_undecomposed_goal`] must invoke under: the
    /// baseline `"assistant.respond"` for every local/mock/self-hosted-engine backend (never
    /// gated), or this provider's own requestable `"cloud.<provider>"` string when a real cloud
    /// backend is active -- so only cloud dispatch is ever gated behind a real user consent.
    /// Kept as a hardcoded literal here, matching how this crate already hardcodes
    /// `"assistant.respond"` rather than importing `hyperion-agent-runtime`'s own (private)
    /// capability-ref constants.
    fn capability_ref(&self) -> &'static str {
        match self {
            BackendKind::Cloud { provider, .. } => provider.capability_ref(),
            _ => "assistant.respond",
        }
    }
}

/// One real, paid, external cloud provider -- see [`BackendKind::Cloud`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloudProvider {
    OpenAi,
    Anthropic,
    Gemini,
    /// Groq's own real, hosted, paid API -- LPU-hosted inference, not self-hosted, so it's a
    /// `CloudProvider` (real consent gate) rather than an `EngineKind` (never gated) despite its
    /// wire protocol being OpenAI-compatible; see [`ConsoleSession::try_connect_groq`].
    Groq,
}

impl CloudProvider {
    /// Also this provider's key in [`ConsoleSession::secret_store`] -- one real, stable name
    /// shared by both the human-facing label and the storage key, so there's no separate mapping
    /// to keep in sync.
    fn label(self) -> &'static str {
        match self {
            CloudProvider::OpenAi => "openai",
            CloudProvider::Anthropic => "anthropic",
            CloudProvider::Gemini => "gemini",
            CloudProvider::Groq => "groq",
        }
    }

    /// Matches the private capability-ref constants `hyperion-agent-runtime`'s own `runtime.rs`
    /// declares (`CLOUD_OPENAI_CAPABILITY` etc.) -- kept as a hardcoded literal here rather than
    /// importing them, exactly as this crate already hardcodes `"assistant.respond"` rather than
    /// importing `ASSISTANT_RESPOND_CAPABILITY`.
    fn capability_ref(self) -> &'static str {
        match self {
            CloudProvider::OpenAi => "cloud.openai",
            CloudProvider::Anthropic => "cloud.anthropic",
            CloudProvider::Gemini => "cloud.gemini",
            CloudProvider::Groq => "cloud.groq",
        }
    }
}

/// One well-known OpenAI-compatible local engine/proxy preset, plus `Custom` for anything else
/// speaking the same protocol -- see [`hyperion_ai_runtime::openai_compat_backend`]'s own doc
/// comment on why one backend implementation covers all of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EngineKind {
    Ollama,
    VLlm,
    LiteLlm,
    /// No preset default base URL -- the caller must give one explicitly.
    Custom,
}

impl EngineKind {
    fn label(self) -> &'static str {
        match self {
            EngineKind::Ollama => "ollama",
            EngineKind::VLlm => "vllm",
            EngineKind::LiteLlm => "litellm",
            EngineKind::Custom => "custom",
        }
    }

    /// Each preset's own well-known documented default port; `None` for `Custom`, which has no
    /// such convention and must always be given a real base URL explicitly.
    fn default_base_url(self) -> Option<&'static str> {
        match self {
            EngineKind::Ollama => Some("http://localhost:11434/v1"),
            EngineKind::VLlm => Some("http://localhost:8000/v1"),
            EngineKind::LiteLlm => Some("http://localhost:4000/v1"),
            EngineKind::Custom => None,
        }
    }

    /// The env var this engine's optional bearer key (if its server needs one at all -- Ollama
    /// and vLLM typically don't; a self-hosted LiteLLM proxy often does) is read from --
    /// namespaced per engine so one provider's key can never leak onto another's connection.
    /// Only used by [`ConsoleSession::try_connect_engine`]'s real (`openai-compat`-gated) arm.
    #[cfg(feature = "openai-compat")]
    fn api_key_env_var(self) -> &'static str {
        match self {
            EngineKind::Ollama => "HYPERION_OLLAMA_API_KEY",
            EngineKind::VLlm => "HYPERION_VLLM_API_KEY",
            EngineKind::LiteLlm => "HYPERION_LITELLM_API_KEY",
            EngineKind::Custom => "HYPERION_CUSTOM_API_KEY",
        }
    }
}

impl ConsoleSession {
    /// `data_dir` is where the real, WAL-backed Knowledge Graph this session's Intent Engine
    /// grounds against lives -- on the real booted image, M6's own dedicated persistent
    /// partition; in a test, any tempdir.
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self, GraphError> {
        let data_dir = data_dir.as_ref();
        // A genuinely fresh install (or a caller-supplied path that doesn't exist yet) used to
        // crash here with a raw "No such file or directory" WAL error -- this crate's own real
        // data directory, never created for the caller, only ever assumed to already exist.
        // Ignoring the result is deliberate, not a silently-swallowed error: if creation somehow
        // still fails (a real permissions/disk problem), `KnowledgeGraph::open`/`Keystore::
        // open_or_create` immediately below hit the exact same underlying failure and surface a
        // real error through this function's own existing `Result`, same as before.
        let _ = std::fs::create_dir_all(data_dir);
        let kg_path = PathBuf::from(data_dir).join("console_knowledge_graph.jsonl");
        let mut monitor = CapabilityMonitor::new();
        let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
        let graph = Arc::new(KnowledgeGraph::open(&kg_path)?);
        let context = Arc::new(hyperion_context::ContextEngine::new(graph.clone()));
        let netstack = Arc::new(Self::build_netstack(graph.clone()));
        let graph_explorer = GraphExplorer::new(graph.clone());
        let intent_engine = IntentEngine::new(graph.clone(), context.clone());

        let keystore = Keystore::open_or_create(&data_dir.join("device.key"))
            .expect("open or create this session's real device signing key");
        let (runtime, current_backend) = Self::build_ai_runtime(&keystore);
        let ai_runtime = Arc::new(runtime);
        let agent_runtime = Arc::new(AgentRuntime::new_with_netstack(
            ai_runtime.clone(),
            Some(netstack.clone()),
        ));
        let coordination = CoordinationSession::new(agent_runtime.clone(), graph);

        let assistant_manifest = hyperion_coordination::default_manifests()
            .into_iter()
            .find(|m| m.specialization == "assistant")
            .expect("default_manifests always includes the assistant specialization");
        let assistant_instance_id = agent_runtime
            .spawn(&monitor, &token, assistant_manifest, None)
            .expect("spawn this session's own persistent assistant Agent instance");

        // Deliberately NOT pre-seeded from an already-connected provider's own prior-session
        // secret: doing so would make `InvokeOutcome::PendingConsent` permanently unreachable
        // through any real console sequence at all (every path to a `Cloud` backend requires a
        // stored secret, and every stored secret would already carry a grant) -- real, tested
        // machinery that a real user could never actually see fire is exactly the "looks real,
        // never actually exercised" gap this workspace's own discipline rules out elsewhere. A
        // stored secret only proves a real account *exists*; [`Self::finish_connect`] grants
        // consent to *use* it immediately within the session that just connected it (so
        // connecting doesn't also demand an immediate, redundant re-confirmation), but a fresh
        // boot's first real cloud dispatch genuinely re-asks -- once per boot, not once per
        // message, and never silently bypassed.
        let secret_store =
            SecretStore::open_or_create(&data_dir.join("cloud_secrets.enc"), &keystore)
                .expect("open or create this session's real encrypted cloud-secret store");

        // A real, permissive domain-egress grant for this session's own root token, minted once
        // here rather than per-call: a real interactive assistant can't pre-enumerate every real
        // domain a user might ask about, so this uses the real "*" wildcard pattern
        // (docs/998-roadmap.md M10 -- see `hyperion_netstack::hub`'s own `domain_matches`
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
            ai_runtime,
            current_backend,
            assistant_instance_id,
            secret_store,
            pending_connect: None,
            pending_consent: None,
            graph_explorer,
            workspace: WorkspaceCompiler::new(),
            session_id: "console".to_string(),
            last_plan_session_id: None,
            keystore,
            pending_think_root: None,
        })
    }

    /// This session's own real, public Ed25519 verifying key -- what a peer's real A2A/MCP
    /// server presents in its own Agent Card/`initialize` response as real, checkable proof of
    /// identity (see [`Self::sign`] for the other half).
    pub fn verifying_key(&self) -> hyperion_crypto::VerifyingKey {
        self.keystore.verifying_key()
    }

    /// A real Ed25519 signature over `bytes`, using this session's own device identity -- what a
    /// real A2A/MCP server signs its own reply payload with, so a caller holding the matching
    /// [`Self::verifying_key`] can really verify the reply came from the entity that presented
    /// that key, not just trust an unauthenticated claim.
    pub fn sign(&self, bytes: &[u8]) -> hyperion_crypto::Signature {
        self.keystore.sign(bytes)
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
    /// docs/998-roadmap.md M9: the registered descriptor is really Ed25519-signed, not a
    /// checksum stand-in -- by this session's own real device identity, a [`Keystore`] persisted
    /// under `data_dir` (the same real, dedicated partition M6 already gives the Knowledge Graph),
    /// so it's stable across reboots rather than a fresh, unverifiable identity every restart.
    fn build_ai_runtime(keystore: &Keystore) -> (LocalAiRuntime, BackendKind) {
        let (backend, current_backend) = if cfg!(feature = "candle") {
            match Self::try_load_candle() {
                Ok(backend) => (backend, BackendKind::Candle),
                Err(e) => {
                    eprintln!(
                        "warning: {e}; falling back to the mock inference backend for this \
                         session"
                    );
                    (
                        Box::new(MockBackend) as Box<dyn hyperion_ai_runtime::InferenceBackend>,
                        BackendKind::Mock,
                    )
                }
            }
        } else {
            (
                Box::new(MockBackend) as Box<dyn hyperion_ai_runtime::InferenceBackend>,
                BackendKind::Mock,
            )
        };

        let runtime = LocalAiRuntime::new(backend, 8_000);
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
        descriptor.signature = Some(sign(&descriptor, keystore));
        runtime
            .register_model(descriptor, &keystore.verifying_key())
            .expect("a descriptor this session just really signed always verifies");
        (runtime, current_backend)
    }

    /// Loads a fresh real [`hyperion_ai_runtime::CandleBackend`], or a clear, honest error if
    /// this binary wasn't even compiled with `--features candle` -- shared by
    /// [`Self::build_ai_runtime`] (startup) and [`Self::switch_backend`] (the `/backend`/
    /// `use backend` meta-command), so both paths give the exact same message for the exact same
    /// failure.
    #[cfg(feature = "candle")]
    fn try_load_candle() -> Result<Box<dyn hyperion_ai_runtime::InferenceBackend>, String> {
        hyperion_ai_runtime::CandleBackend::load()
            .map(|backend| Box::new(backend) as Box<dyn hyperion_ai_runtime::InferenceBackend>)
            .map_err(|e| format!("real Candle model load failed ({e})"))
    }

    #[cfg(not(feature = "candle"))]
    fn try_load_candle() -> Result<Box<dyn hyperion_ai_runtime::InferenceBackend>, String> {
        Err(
            "this build wasn't compiled with real inference support (--features candle)"
                .to_string(),
        )
    }

    /// As [`Self::try_load_candle`], for the `openai-compat` feature's real
    /// [`hyperion_ai_runtime::OpenAiCompatBackend`] -- shared by [`Self::switch_backend`]'s
    /// `Engine` arm. Unlike Candle there's no startup-time counterpart: an engine backend is
    /// only ever reached via an explicit `/backend`/`use backend` switch after boot, never
    /// auto-selected (see [`Self::build_ai_runtime`], which stays candle-or-mock-only).
    #[cfg(feature = "openai-compat")]
    fn try_connect_engine(
        engine: EngineKind,
        base_url: &str,
        model: &str,
    ) -> Result<Box<dyn hyperion_ai_runtime::InferenceBackend>, String> {
        let api_key = std::env::var(engine.api_key_env_var()).ok();
        hyperion_ai_runtime::OpenAiCompatBackend::connect(base_url, model, api_key)
            .map(|backend| Box::new(backend) as Box<dyn hyperion_ai_runtime::InferenceBackend>)
            .map_err(|e| {
                format!(
                    "couldn't connect to the real {} server: {e}",
                    engine.label()
                )
            })
    }

    #[cfg(not(feature = "openai-compat"))]
    fn try_connect_engine(
        _engine: EngineKind,
        _base_url: &str,
        _model: &str,
    ) -> Result<Box<dyn hyperion_ai_runtime::InferenceBackend>, String> {
        Err("this build wasn't compiled with real local-engine support \
             (--features openai-compat)"
            .to_string())
    }

    /// The `Cloud` backend-switch arm's real effect: looks up `provider`'s real API key in
    /// [`Self::secret_store`], erroring with a clear next step if it isn't there yet, then
    /// connects via that provider's own real backend.
    fn try_connect_cloud(
        &self,
        provider: CloudProvider,
        model: &str,
    ) -> Result<Box<dyn hyperion_ai_runtime::InferenceBackend>, String> {
        let Some(api_key) = self.secret_store.get(provider.label()) else {
            return Err(format!(
                "you haven't connected your {} account yet -- try \"connect my {} account\"",
                provider.label(),
                provider.label()
            ));
        };

        match provider {
            CloudProvider::OpenAi => Self::try_connect_openai(api_key, model),
            CloudProvider::Anthropic => Self::try_connect_anthropic(api_key, model),
            CloudProvider::Gemini => Self::try_connect_gemini(api_key, model),
            CloudProvider::Groq => Self::try_connect_groq(api_key, model),
        }
    }

    /// OpenAI's own real API already speaks the OpenAI-compatible shape
    /// [`hyperion_ai_runtime::OpenAiCompatBackend`] covers -- no new backend needed, just its
    /// real, fixed base URL.
    #[cfg(feature = "openai-compat")]
    fn try_connect_openai(
        api_key: &str,
        model: &str,
    ) -> Result<Box<dyn hyperion_ai_runtime::InferenceBackend>, String> {
        // A real feature, not just a testing seam: an Azure OpenAI deployment or a corporate
        // proxy in front of the real API speaks the same OpenAI-compatible shape at a different
        // base URL. Defaults to the real API when unset, exactly as every real caller expects.
        let base_url = std::env::var("HYPERION_OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        hyperion_ai_runtime::OpenAiCompatBackend::connect(
            base_url,
            model,
            Some(api_key.to_string()),
        )
        .map(|backend| Box::new(backend) as Box<dyn hyperion_ai_runtime::InferenceBackend>)
        .map_err(|e| format!("couldn't connect to the real OpenAI API: {e}"))
    }

    #[cfg(not(feature = "openai-compat"))]
    fn try_connect_openai(
        _api_key: &str,
        _model: &str,
    ) -> Result<Box<dyn hyperion_ai_runtime::InferenceBackend>, String> {
        Err(
            "this build wasn't compiled with real OpenAI-compatible support \
             (--features openai-compat)"
                .to_string(),
        )
    }

    /// Groq's own real API speaks the same OpenAI-compatible shape
    /// [`hyperion_ai_runtime::OpenAiCompatBackend`] covers (unlike Anthropic/Gemini below, which
    /// each need their own dedicated backend) -- no new backend needed, just Groq's own real
    /// base URL. Gated on `openai-compat`, not a new feature, for that reason: this is the exact
    /// same wire protocol the cloud-OpenAI and local-engine arms already speak.
    /// `HYPERION_GROQ_BASE_URL` overrides the default exactly as `HYPERION_OPENAI_BASE_URL` does
    /// for OpenAI -- see [`Self::try_connect_openai`]'s own doc comment for why that's a real
    /// feature (a corporate proxy in front of Groq's real API) and not just a testing seam.
    #[cfg(feature = "openai-compat")]
    fn try_connect_groq(
        api_key: &str,
        model: &str,
    ) -> Result<Box<dyn hyperion_ai_runtime::InferenceBackend>, String> {
        let base_url = std::env::var("HYPERION_GROQ_BASE_URL")
            .unwrap_or_else(|_| "https://api.groq.com/openai/v1".to_string());
        hyperion_ai_runtime::OpenAiCompatBackend::connect(
            base_url,
            model,
            Some(api_key.to_string()),
        )
        .map(|backend| Box::new(backend) as Box<dyn hyperion_ai_runtime::InferenceBackend>)
        .map_err(|e| format!("couldn't connect to the real Groq API: {e}"))
    }

    #[cfg(not(feature = "openai-compat"))]
    fn try_connect_groq(
        _api_key: &str,
        _model: &str,
    ) -> Result<Box<dyn hyperion_ai_runtime::InferenceBackend>, String> {
        Err(
            "this build wasn't compiled with real OpenAI-compatible support \
             (--features openai-compat)"
                .to_string(),
        )
    }

    #[cfg(feature = "anthropic")]
    fn try_connect_anthropic(
        api_key: &str,
        model: &str,
    ) -> Result<Box<dyn hyperion_ai_runtime::InferenceBackend>, String> {
        hyperion_ai_runtime::AnthropicBackend::connect(api_key, model)
            .map(|backend| Box::new(backend) as Box<dyn hyperion_ai_runtime::InferenceBackend>)
            .map_err(|e| format!("couldn't connect to the real Anthropic API: {e}"))
    }

    #[cfg(not(feature = "anthropic"))]
    fn try_connect_anthropic(
        _api_key: &str,
        _model: &str,
    ) -> Result<Box<dyn hyperion_ai_runtime::InferenceBackend>, String> {
        Err(
            "this build wasn't compiled with real Anthropic support (--features anthropic)"
                .to_string(),
        )
    }

    #[cfg(feature = "gemini")]
    fn try_connect_gemini(
        api_key: &str,
        model: &str,
    ) -> Result<Box<dyn hyperion_ai_runtime::InferenceBackend>, String> {
        hyperion_ai_runtime::GeminiBackend::connect(api_key, model)
            .map(|backend| Box::new(backend) as Box<dyn hyperion_ai_runtime::InferenceBackend>)
            .map_err(|e| format!("couldn't connect to the real Gemini API: {e}"))
    }

    #[cfg(not(feature = "gemini"))]
    fn try_connect_gemini(
        _api_key: &str,
        _model: &str,
    ) -> Result<Box<dyn hyperion_ai_runtime::InferenceBackend>, String> {
        Err("this build wasn't compiled with real Gemini support (--features gemini)".to_string())
    }

    /// The `/backend <name> [args...]` / `use backend <name> [args...]` meta-command's real
    /// effect: swaps [`Self::ai_runtime`]'s live backend in place via
    /// [`hyperion_ai_runtime::LocalAiRuntime::set_backend`] -- no restart, no new session, every
    /// other piece of state (Knowledge Graph, capability token, registered model descriptor)
    /// untouched. A no-op (with its own honest reply) if `kind` is already active, so repeating
    /// the command is always safe.
    fn switch_backend(&mut self, kind: BackendKind) -> String {
        if kind == self.current_backend {
            return format!("Already using the {} backend.", kind.label());
        }
        let backend = match &kind {
            BackendKind::Mock => {
                Box::new(MockBackend) as Box<dyn hyperion_ai_runtime::InferenceBackend>
            }
            BackendKind::Candle => match Self::try_load_candle() {
                Ok(backend) => backend,
                Err(e) => return format!("I couldn't switch: {e}."),
            },
            BackendKind::Engine {
                engine,
                base_url,
                model,
            } => match Self::try_connect_engine(*engine, base_url, model) {
                Ok(backend) => backend,
                Err(e) => return format!("I couldn't switch: {e}."),
            },
            BackendKind::Cloud { provider, model } => {
                match self.try_connect_cloud(*provider, model) {
                    Ok(backend) => backend,
                    Err(e) => return format!("I couldn't switch: {e}."),
                }
            }
        };
        self.ai_runtime.set_backend(backend);
        let label = kind.label();
        self.current_backend = kind;
        format!("Switched to the {label} backend.")
    }

    /// Recognizes the console's small set of meta-commands -- session/runtime controls, not
    /// goals -- ahead of the intent engine, the same "deterministic check before the AI path"
    /// tier [`Self::run_undecomposed_goal`]'s own URL check already uses. Both a `/`-prefixed
    /// form and a plain-English `"use backend "` phrase are recognized for the backend switch;
    /// the plain-English form is deliberately the full three words "use backend", never the bare
    /// "use <name>" -- "candle" and "mock" are ordinary enough words that a two-word phrase could
    /// collide with a real goal utterance, exactly the ambiguity a meta-command must never risk.
    /// Returns `None` (not a meta-command at all) for everything else, so the normal goal
    /// pipeline runs unchanged.
    fn handle_meta_command(&mut self, utterance: &str) -> Option<Vec<String>> {
        let trimmed = utterance.trim();
        let lower = trimmed.to_ascii_lowercase();

        // Plain "help" (no slash) used to silently fall through to a real Agent dispatch --
        // the single most natural thing a lost or new user would type got echoed back by
        // whichever backend was active instead of ever reaching this crate's own, already
        // well-designed help text.
        if lower == "/help" || lower == "help" {
            return Some(Self::help_text());
        }

        // docs/998-roadmap.md's Backlog "Protect the Human" item: an opt-in, per-session pause
        // before Hyperion decomposes a goal -- "a moment for the human's own reasoning to run
        // first," never a default. `/think`/`/think on`/`/think off` toggle it;
        // `/think-proceed` commits to the most recent real pending decomposition once that
        // reasoning has run.
        if lower.starts_with("/think-proceed") {
            let Some(root) = self.pending_think_root.take() else {
                return Some(vec![
                    "Nothing is paused waiting on \"/think-proceed\" right now.".to_string(),
                ]);
            };
            return Some(
                match self.intent_engine.proceed_with_decomposition(
                    &self.monitor,
                    &self.token,
                    root,
                ) {
                    Ok(_) => vec!["Proceeding -- decomposing that goal now.".to_string()],
                    Err(e) => vec![format!("I couldn't proceed with that: {e}")],
                },
            );
        }
        if lower.starts_with("/think") {
            let rest = trimmed["/think".len()..].trim();
            return Some(match rest {
                "" => vec![format!(
                    "Think mode is currently {} for this session.",
                    if self.intent_engine.is_think_mode(&self.session_id) {
                        "on"
                    } else {
                        "off"
                    }
                )],
                "on" => {
                    self.intent_engine.set_think_mode(&self.session_id, true);
                    vec![
                        "Think mode is on -- I'll pause before deciding what a new goal means \
                         until you say \"/think-proceed\"."
                            .to_string(),
                    ]
                }
                "off" => {
                    self.intent_engine.set_think_mode(&self.session_id, false);
                    self.pending_think_root = None;
                    vec!["Think mode is off.".to_string()]
                }
                other => vec![format!(
                    "\"/think {other}\" isn't a mode I know -- try \"/think on\" or \"/think off\"."
                )],
            });
        }

        if lower.starts_with("/recall") {
            let text = trimmed["/recall".len()..].trim();
            return Some(self.graph_explorer.recall(&self.monitor, &self.token, text));
        }

        if lower.starts_with("/why") {
            let rest = trimmed["/why".len()..].trim();
            return Some(match rest.parse::<usize>() {
                Ok(n) => self.graph_explorer.why(&self.monitor, &self.token, n),
                Err(_) => vec![
                    "\"/why\" needs a result number from a recent \"/recall\" or \"/related\" \
                     -- try \"/why 1\"."
                        .to_string(),
                ],
            });
        }

        if lower.starts_with("/related") {
            let rest = trimmed["/related".len()..].trim();
            return Some(match rest.parse::<usize>() {
                Ok(n) => self.graph_explorer.related(&self.monitor, &self.token, n),
                Err(_) => vec![
                    "\"/related\" needs a result number from a recent \"/recall\" or \
                     \"/related\" -- try \"/related 1\"."
                        .to_string(),
                ],
            });
        }

        if lower.starts_with("/result") {
            let task_name = trimmed["/result".len()..].trim();
            if task_name.is_empty() {
                return Some(vec![
                    "\"/result\" needs a task name, e.g. \"/result market_research\".".to_string(),
                ]);
            }
            return Some(
                self.graph_explorer
                    .result(&self.monitor, &self.token, task_name),
            );
        }

        if lower.starts_with("/graph") {
            let rest = trimmed["/graph".len()..].trim();
            let as_dot = match rest {
                "" => false,
                "dot" => true,
                other => {
                    return Some(vec![format!(
                        "\"/graph {other}\" isn't a format I know -- try \"/graph\" or \"/graph \
                         dot\"."
                    )])
                }
            };
            return Some(
                self.graph_explorer
                    .dump_graph(&self.monitor, &self.token, as_dot),
            );
        }

        if lower.starts_with("connect") {
            for provider in [
                CloudProvider::OpenAi,
                CloudProvider::Anthropic,
                CloudProvider::Gemini,
                CloudProvider::Groq,
            ] {
                if lower.contains(provider.label()) {
                    self.pending_connect = Some(provider);
                    return Some(vec![format!(
                        "Paste your {} API key (it won't be echoed or logged):",
                        provider.label()
                    )]);
                }
            }
            return Some(vec![
                "Connect which provider? Try \"connect my openai account\", \"connect my \
                 anthropic account\", \"connect my gemini account\", or \"connect my groq \
                 account\"."
                    .to_string(),
            ]);
        }

        let arg = if lower.starts_with("/backend") {
            trimmed["/backend".len()..].trim()
        } else if lower.starts_with("use backend") {
            trimmed["use backend".len()..].trim()
        } else if trimmed.starts_with('/') {
            // A mistyped or unrecognized slash command used to silently fall through to a real
            // Agent dispatch, sent to whichever backend was active as if it were a genuine
            // goal -- a typo got an unrelated, confusing "response" instead of any feedback
            // that the command itself wasn't recognized.
            return Some(vec![format!(
                "I don't recognize \"{trimmed}\" as a command -- try \"/help\" to see what's \
                 available."
            )]);
        } else {
            return None;
        };

        if arg.is_empty() {
            return Some(vec![format!(
                "Currently using the {} backend.",
                self.current_backend.label()
            )]);
        }

        let mut tokens = arg.split_whitespace();
        let kind_name = tokens.next().unwrap_or_default().to_ascii_lowercase();
        let rest: Vec<&str> = tokens.collect();

        let kind = match kind_name.as_str() {
            "candle" | "real" | "llama" => BackendKind::Candle,
            "mock" | "echo" => BackendKind::Mock,
            "ollama" | "vllm" | "litellm" | "custom" => {
                let engine = match kind_name.as_str() {
                    "ollama" => EngineKind::Ollama,
                    "vllm" => EngineKind::VLlm,
                    "litellm" => EngineKind::LiteLlm,
                    _ => EngineKind::Custom,
                };
                match Self::parse_engine_args(engine, &rest) {
                    Ok((base_url, model)) => BackendKind::Engine {
                        engine,
                        base_url,
                        model,
                    },
                    Err(e) => return Some(vec![e]),
                }
            }
            "openai" | "anthropic" | "gemini" | "groq" => {
                let provider = match kind_name.as_str() {
                    "openai" => CloudProvider::OpenAi,
                    "anthropic" => CloudProvider::Anthropic,
                    "gemini" => CloudProvider::Gemini,
                    _ => CloudProvider::Groq,
                };
                match rest.as_slice() {
                    [model] => BackendKind::Cloud {
                        provider,
                        model: model.to_string(),
                    },
                    _ => {
                        return Some(vec![format!(
                            "\"{}\" needs a model name: /backend {} <model>",
                            provider.label(),
                            provider.label()
                        )])
                    }
                }
            }
            other => {
                return Some(vec![format!(
                    "I don't know a \"{other}\" backend -- try \"candle\", \"mock\", \
                     \"ollama\", \"vllm\", \"litellm\", \"custom\", \"openai\", \"anthropic\", \
                     \"gemini\", or \"groq\"."
                )])
            }
        };

        Some(vec![self.switch_backend(kind)])
    }

    /// Parses the `<model> [base_url]` shape (preset engines, `base_url` optional -- see
    /// [`EngineKind::default_base_url`]) or `<base_url> <model>` shape (`custom`, no preset,
    /// both required) for one engine kind. `base_url` is normalized (trailing slash trimmed)
    /// here, the one place both call paths (preset and custom) funnel through.
    fn parse_engine_args(engine: EngineKind, args: &[&str]) -> Result<(String, String), String> {
        match engine {
            EngineKind::Custom => match args {
                [base_url, model] => Ok((
                    base_url.trim_end_matches('/').to_string(),
                    model.to_string(),
                )),
                _ => Err("\"custom\" needs both a base URL and a model name: \
                     /backend custom <base_url> <model>"
                    .to_string()),
            },
            _ => match args {
                [model] => Ok((
                    engine
                        .default_base_url()
                        .expect("preset engines always have a default base_url")
                        .to_string(),
                    model.to_string(),
                )),
                [model, base_url] => Ok((
                    base_url.trim_end_matches('/').to_string(),
                    model.to_string(),
                )),
                _ => Err(format!(
                    "\"{}\" needs a model name: /backend {} <model> [base_url]",
                    engine.label(),
                    engine.label()
                )),
            },
        }
    }

    fn help_text() -> Vec<String> {
        vec![
            "I'm not menu-driven -- just tell me what you'd like to do, in your own words."
                .to_string(),
            String::new(),
            "A couple of things you can also ask directly:".to_string(),
            "  /backend <candle|mock>                     switch to a built-in backend (also: \
             \"use backend <name>\")"
                .to_string(),
            "  /backend <ollama|vllm|litellm> <model> [base_url]".to_string(),
            "                                              switch to a real local engine or proxy"
                .to_string(),
            "  /backend custom <base_url> <model>         switch to any other \
             OpenAI-compatible server"
                .to_string(),
            "  /backend <openai|anthropic|gemini|groq> <model>  switch to a real cloud \
             provider (needs a connected account)"
                .to_string(),
            "  /backend                                    show which backend is active right \
             now"
            .to_string(),
            "  connect my <provider> account                store a real API key for openai, \
             anthropic, gemini, or groq"
                .to_string(),
            "  /recall [text]                              look through what I've recorded \
             (bare, for everything recent)"
                .to_string(),
            "  /why <n>                                    explain a \"/recall\"/\"/related\" \
             result -- when it was recorded, how connected it is"
                .to_string(),
            "  /related <n>                                show what's connected to a \
             \"/recall\"/\"/related\" result"
                .to_string(),
            "  /result <task>                              show a task's real, complete result \
             directly, e.g. \"/result market_research\""
                .to_string(),
            "  /redo <task> <extra instructions>           redo a task from the last plan with \
             more information, e.g. \"/redo market_research focus on Europe\""
                .to_string(),
            "  /graph                                       dump everything recorded, as text \
             (run before/after to diff what changed)"
                .to_string(),
            "  /graph dot                                  the same, as Graphviz DOT -- pipe to \
             \"dot -Tsvg\" to draw it"
                .to_string(),
            "  /think on|off                                pause before deciding what a new \
             goal means, until you say \"/think-proceed\" (bare, to check the current mode)"
                .to_string(),
            "  /think-proceed                              commit to deciding what the paused \
             goal means"
                .to_string(),
            "  /help                                        show this message".to_string(),
        ]
    }

    /// Real network selection for this session's own `web.research` calls (see
    /// [`Self::run_web_research`]) -- a real [`hyperion_netstack::ReqwestFetchBackend`] +
    /// [`hyperion_netstack::HtmlHeuristicExtractionBackend`] when this binary is built with
    /// `--features real-http`, [`hyperion_netstack::MockFetchBackend`]/
    /// [`hyperion_netstack::MockExtractionBackend`] otherwise -- the exact same "swap the
    /// backend, not the call site" principle [`Self::build_ai_runtime`] already established for
    /// M8 (docs/998-roadmap.md M10). Off by default for the same reason: a real release
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
    ///
    /// A thin wrapper over [`Self::handle_utterance_with_progress`] with a no-op progress
    /// callback -- this crate's own tests all use this, since a decomposed plan's *final*
    /// rendered text is identical either way; only `main.rs` needs to see intermediate lines as
    /// they happen.
    pub fn handle_utterance(&mut self, utterance: &str) -> Vec<String> {
        self.handle_utterance_with_progress(utterance, &mut |_| {})
    }

    /// As [`Self::handle_utterance`], but calls `on_progress` with a real [`TaskProgress`] event
    /// once per tick of a decomposed multi-task plan (see [`Self::run_decomposed_plan`]): a
    /// `Starting` event naming every task about to be dispatched, *before* the real (potentially
    /// slow) capability call blocks this thread, then one `Done` event per task once that same
    /// blocking call returns. A real, previously-shipped UX gap this fixes: a real capability
    /// dispatch can be a real, slow network call to a real cloud model, and this console used to
    /// print nothing at all -- not even "this is running" -- while a multi-task plan
    /// (`market_research` -> `{business_model, branding}` -> `legal_formation`, this workspace's
    /// own built-in template) worked through several such calls in sequence.
    pub fn handle_utterance_with_progress(
        &mut self,
        utterance: &str,
        on_progress: &mut dyn FnMut(TaskProgress),
    ) -> Vec<String> {
        if let Some(provider) = self.pending_connect.take() {
            return self.finish_connect(provider, utterance);
        }
        if let Some(pending) = self.pending_consent.take() {
            return self.finish_consent(pending, utterance);
        }
        let trimmed = utterance.trim();
        if trimmed.to_ascii_lowercase().starts_with("/redo") {
            let rest = trimmed["/redo".len()..].trim();
            let (task_name, extra_context) = match rest.split_once(char::is_whitespace) {
                Some((name, ctx)) => (name.trim(), ctx.trim()),
                None => (rest, ""),
            };
            if task_name.is_empty() {
                return vec![
                    "\"/redo\" needs a task name, e.g. \"/redo market_research focus on the \
                     European market only\"."
                        .to_string(),
                ];
            }
            return self.redo_task(task_name, extra_context.to_string(), on_progress);
        }
        if let Some(reply) = self.handle_meta_command(utterance) {
            return reply;
        }

        let outcome = match self.intent_engine.handle_utterance(
            &self.monitor,
            &self.token,
            utterance,
            &self.session_id,
        ) {
            Ok(outcome) => outcome,
            Err(e) => return vec![format!("I couldn't understand that: {e}")],
        };

        let root = match outcome {
            HandleOutcome::Submitted(root) => root,
            HandleOutcome::PendingThink(root) => {
                self.pending_think_root = Some(root);
                return vec![
                    "Pausing before I decide what that means -- take a moment, then say \
                     \"/think-proceed\" when you're ready."
                        .to_string(),
                ];
            }
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
            self.run_decomposed_plan(&ticket, on_progress)
        };

        self.render_workspace(root, &self.session_id, &predicate, &outcomes)
    }

    /// `true` while a "connect my `<provider>`" flow is awaiting its follow-up API-key line --
    /// `main.rs` checks this before every real `read_line` so that one line (and only that one)
    /// isn't echoed to the terminal or left in scrollback.
    pub fn awaiting_secret_input(&self) -> bool {
        self.pending_connect.is_some()
    }

    /// The real second half of "connect my `<provider>` account": `api_key_line` is the raw next
    /// line the user typed (with no-echo already handled by the caller, per
    /// [`Self::awaiting_secret_input`]) -- stores it for real in [`Self::secret_store`] and
    /// immediately grants this *one running session* the right to use it (via
    /// [`hyperion_agent_runtime::AgentRuntime::grant_capability`]), so typing the key doesn't
    /// also demand an immediate, redundant `PendingConsent` round trip right after connecting.
    /// This grant does NOT persist to the next boot -- see [`Self::secret_store`]'s own doc
    /// comment on why a fresh restart still re-asks once, for real, on first real use.
    fn finish_connect(&mut self, provider: CloudProvider, api_key_line: &str) -> Vec<String> {
        // A real API key never legitimately contains a control character -- stripping them
        // defends against a real, observed failure mode: some terminals wrap pasted text in
        // escape sequences (bracketed-paste markers) or leave a stray `\r`, which `.trim()`
        // alone doesn't catch (it only trims Unicode *whitespace*, and ESC/CR aren't
        // whitespace). A key silently corrupted this way looks fine to type but fails real
        // authentication with a real, honest 401 -- exactly the report that motivated this.
        let api_key: String = api_key_line
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        if api_key.is_empty() {
            return vec![format!(
                "No key entered -- not connecting your {} account.",
                provider.label()
            )];
        }
        if let Err(e) = self.secret_store.set(provider.label(), &api_key) {
            return vec![format!("I couldn't save that key: {e}")];
        }
        let _ = self.agent_runtime.grant_capability(
            &self.monitor,
            &self.token,
            self.assistant_instance_id,
            provider.capability_ref(),
        );
        vec![format!(
            "Connected ({}, {} characters). I can use {} now when it's the best fit -- try \
             \"/backend {} <model>\".",
            mask_secret(&api_key),
            api_key.chars().count(),
            provider.label(),
            provider.label()
        )]
    }

    /// The real other half of a live `InvokeOutcome::PendingConsent` -- `answer` is the raw next
    /// utterance the user typed in response to [`Self::run_undecomposed_goal`]'s own consent
    /// prompt. On a real "yes", resolves the consent for real
    /// ([`hyperion_agent_runtime::AgentRuntime::resolve_consent`]) and re-invokes the exact same
    /// prompt that originally triggered it, now `Granted`. Bypasses [`Self::render_workspace`]
    /// entirely (like every other meta-command reply) -- this confirmation is its own turn, not
    /// a continuation of the turn that first hit `PendingConsent`.
    fn finish_consent(&mut self, pending: PendingCloudConsent, answer: &str) -> Vec<String> {
        let approved = matches!(answer.trim().to_ascii_lowercase().as_str(), "yes" | "y");

        if let Err(e) = self.agent_runtime.resolve_consent(
            &self.monitor,
            &self.token,
            self.assistant_instance_id,
            approved,
        ) {
            return vec![format!("Something went wrong resolving that: {e}")];
        }
        if !approved {
            return vec!["Okay -- I won't use that provider for this.".to_string()];
        }

        let args = serde_json::json!({ "prompt": pending.prompt });
        match self.agent_runtime.invoke(
            &self.monitor,
            &self.token,
            self.assistant_instance_id,
            &pending.capability_ref,
            args,
        ) {
            Ok(InvokeOutcome::Result(value)) => {
                let text = value.get("text").and_then(|v| v.as_str()).unwrap_or("");
                vec![format!("done -- {text}")]
            }
            Ok(InvokeOutcome::Denied) => vec!["denied".to_string()],
            Ok(InvokeOutcome::PendingConsent) => {
                vec!["something went wrong -- still pending consent after approving".to_string()]
            }
            Ok(InvokeOutcome::QuotaExceeded) => vec!["over quota right now".to_string()],
            Ok(InvokeOutcome::Failed(reason)) => vec![format!("failed -- {reason}")],
            Err(e) => vec![format!("failed -- {e}")],
        }
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
    /// docs/998-roadmap.md M8: this default action is the "assistant" specialization's
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
    /// docs/998-roadmap.md M10: an utterance containing something that looks like a real
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

        // docs/998-roadmap.md "Phase 2: cloud providers": which real Capability this
        // dispatches under depends on the currently-active backend -- the baseline
        // `"assistant.respond"` for local/mock/self-hosted-engine use (never gated), or a real
        // cloud provider's own requestable `"cloud.<provider>"` string, gated behind a real
        // consent prompt below. Reuses `self.assistant_instance_id` (spawned once in
        // `Self::open`) rather than spawning fresh -- required for any grant, cloud consent
        // included, to ever survive past this one turn.
        let capability_ref = self.current_backend.capability_ref();
        let prompt = self.prompt_with_recent_history(utterance);
        let args = serde_json::json!({ "prompt": prompt });
        let detail = match self.agent_runtime.invoke(
            &self.monitor,
            &self.token,
            self.assistant_instance_id,
            capability_ref,
            args,
        ) {
            Ok(InvokeOutcome::Result(value)) => {
                let text = value.get("text").and_then(|v| v.as_str()).unwrap_or("");
                format!("done -- {text}")
            }
            Ok(InvokeOutcome::Denied) => "denied".to_string(),
            Ok(InvokeOutcome::PendingConsent) => {
                let provider_label = self.current_backend.label();
                self.pending_consent = Some(PendingCloudConsent {
                    capability_ref: capability_ref.to_string(),
                    prompt,
                });
                format!(
                    "This would send your message to a real, paid, external {provider_label} \
                     API -- proceed? (yes/no)"
                )
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

    /// docs/06's "avoid forcing users to repeat themselves," applied to this console's one real
    /// chat path: prefixes `utterance` with this session's own recent conversation, so a genuine
    /// follow-up ("what is my name" after "my name is Alex") gives the model something to
    /// actually answer from. Real now that [`Self::session_id`] is stable across turns (see this
    /// struct's own doc comment) -- `IntentEngine::handle_utterance` already pushed `utterance`
    /// as this working memory's own last entry before this ever runs, so it's excluded here and
    /// asked as the real instruction instead of recapped as history. A session with no prior
    /// turns (or exactly one, the current one) is unchanged from before this existed: bare
    /// `utterance`, nothing prepended.
    fn prompt_with_recent_history(&self, utterance: &str) -> String {
        let history = self.intent_engine.working_memory_turns(&self.session_id);
        let prior = &history[..history.len().saturating_sub(1)];
        if prior.is_empty() {
            return utterance.to_string();
        }
        format!(
            "Recent conversation, most recent last:\n{}\n\nNow respond to: {utterance}",
            prior.join("\n")
        )
    }

    /// docs/998-roadmap.md M10's real deliverable: a URL-shaped undecomposed goal drives
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
        on_progress: &mut dyn FnMut(TaskProgress),
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

        self.last_plan_session_id = Some(session_id);
        self.drive_ticks_to_completion(session_id, on_progress);

        let outcomes = match self
            .coordination
            .get_plan(&self.monitor, &self.token, session_id)
        {
            Ok(plan) => plan
                .nodes
                .iter()
                .map(|node| TaskOutcome {
                    predicate: node.description.clone(),
                    detail: Self::render_task_detail(node),
                })
                .collect(),
            Err(e) => vec![TaskOutcome {
                predicate: "plan".to_string(),
                detail: format!("plan completed but couldn't be read back: {e}"),
            }],
        };

        ("plan".to_string(), outcomes)
    }

    /// Drives every dependency wave of `session_id`'s own real plan to completion -- each
    /// `allocate` call is one real tick (its own ready tasks dispatched concurrently, not one at
    /// a time -- see `hyperion_coordination::CoordinationSession::allocate`'s own doc comment);
    /// an empty result means every task is Done, Failed, or Blocked with nothing left ready.
    /// Shared by [`Self::run_decomposed_plan`] (a plan's first run) and [`Self::redo_task`] (a
    /// `/redo`'d task's own re-run) -- both need the exact same tick-and-report loop, just
    /// starting from a different plan state.
    ///
    /// `on_progress` fires `Starting` *before* each tick's real (potentially slow, blocking)
    /// dispatch -- naming every task this tick is about to run concurrently, via the real
    /// `ready_task_descriptions` peek -- and `Done` once per task after that same blocking call
    /// returns. A real, previously-shipped UX gap this fixes: this console used to stay
    /// completely silent (not even "this is running") until the *entire* plan converged -- a
    /// real user actually hit and reported this.
    fn drive_ticks_to_completion(
        &mut self,
        session_id: u64,
        on_progress: &mut dyn FnMut(TaskProgress),
    ) {
        loop {
            match self
                .coordination
                .ready_task_descriptions(&self.monitor, &self.token, session_id)
            {
                Ok(ready) if !ready.is_empty() => on_progress(TaskProgress::Starting(ready)),
                _ => {}
            }

            match self
                .coordination
                .allocate(&self.monitor, &self.token, session_id)
            {
                Ok(records) if records.is_empty() => break,
                Ok(records) => {
                    if let Ok(plan) =
                        self.coordination
                            .get_plan(&self.monitor, &self.token, session_id)
                    {
                        for record in &records {
                            if let Some(node) =
                                plan.nodes.iter().find(|n| n.task_id == record.task_id)
                            {
                                on_progress(TaskProgress::Done(format!(
                                    "  {}: {:?}",
                                    node.description, node.status
                                )));
                            }
                        }
                    }
                }
                Err(_) => break,
            }
        }
    }

    /// The real second half of `/redo <task> <extra instructions>`: resets `task_name` back to
    /// `Unassigned` with `extra_context` recorded for its next dispatch (via
    /// `hyperion_coordination::CoordinationSession::amend_task`), then re-drives this plan's real
    /// ticks to completion exactly as its first run did -- `task_name` has no unmet dependency
    /// (it just ran), so the very next tick picks it straight back up. Requires
    /// [`Self::last_plan_session_id`] to already be set -- gives an honest "nothing to redo yet"
    /// reply, not a panic or a silently-ignored command, if no decomposed plan has run this
    /// session.
    fn redo_task(
        &mut self,
        task_name: &str,
        extra_context: String,
        on_progress: &mut dyn FnMut(TaskProgress),
    ) -> Vec<String> {
        let Some(session_id) = self.last_plan_session_id else {
            return vec![
                "There's no plan to redo yet -- run a multi-task goal first (e.g. \"I need to \
                 launch my startup\")."
                    .to_string(),
            ];
        };

        let dependents = match self.coordination.amend_task(
            &self.monitor,
            &self.token,
            session_id,
            task_name,
            extra_context,
        ) {
            Ok(dependents) => dependents,
            Err(hyperion_coordination::CoordError::TaskNotFound) => {
                return vec![format!(
                    "I don't have a task called \"{task_name}\" in the last plan."
                )]
            }
            Err(e) => return vec![format!("I couldn't redo \"{task_name}\": {e}")],
        };

        self.drive_ticks_to_completion(session_id, on_progress);

        let mut lines = match self
            .coordination
            .get_plan(&self.monitor, &self.token, session_id)
        {
            Ok(plan) => match plan
                .nodes
                .iter()
                .find(|n| n.description.eq_ignore_ascii_case(task_name))
            {
                Some(node) => vec![format!(
                    "  {}: {}",
                    node.description,
                    Self::render_task_detail(node)
                )],
                None => vec![format!(
                    "Redone, but couldn't find \"{task_name}\" in the plan afterward."
                )],
            },
            Err(e) => vec![format!("Redone, but couldn't read the plan back: {e}")],
        };

        if !dependents.is_empty() {
            lines.push(format!(
                "Note: {} already used the old result and won't be redone automatically -- \
                 \"/redo\" them too if you want them updated.",
                dependents.join(", ")
            ));
        }

        lines
    }

    /// `node.status` alone (`"Done"`) used to be the only real content this console could ever
    /// show for a task: `hyperion-coordination::allocate` discarded a real capability's own real
    /// output the instant it came back -- a real, previously-shipped bug, not a design choice
    /// (see that crate's own doc comment on the "launch my startup produces zero real content"
    /// gap this fixed). `TaskNode.result` now carries it, so a completed task's own real,
    /// generated content is shown right alongside its status -- `"Done -- <preview>"` rather than
    /// a bare status word -- while every other status (still in progress, blocked, failed)
    /// renders exactly as it always did.
    ///
    /// Deliberately a short preview, not the full text: a real cloud model's own real answer to
    /// "draft a business model" can run to several paragraphs, and printing every task's full
    /// text inline would flood the terminal the moment a real plan with several tasks completes.
    /// The full text isn't lost -- `/result <task>` shows it directly, since `allocate` already
    /// records it as its own real, linked `"task_result"` node. Deliberately points at `/result`,
    /// not `/recall <task>` -> `/why <n>`: a real model's own generated prose often naturalizes a
    /// task's snake_case predicate into plain words (e.g. `legal_formation`'s own real result
    /// talks about "legal formation," never the literal string), so a plain text search can miss
    /// the very result a user is looking for -- `/result` finds it via the real graph edge
    /// instead.
    fn render_task_detail(node: &TaskNode) -> String {
        match node
            .result
            .as_ref()
            .and_then(crate::graph_explorer::render_capability_result)
        {
            Some(text) => format!(
                "{:?} -- {} (see \"/result {}\" for the full text)",
                node.status,
                crate::graph_explorer::preview(&text),
                node.description
            ),
            None => format!("{:?}", node.status),
        }
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
        session_id: &str,
        predicate: &str,
        outcomes: &[TaskOutcome],
    ) -> Vec<String> {
        // A decomposed plan's own real, meaningful task names (e.g. "market_research") are
        // genuinely worth prefixing -- with several outcomes rendered together, naming which is
        // which is real information. `"generic_goal"` is different: the internal sentinel
        // `run_undecomposed_goal`/`run_web_research` use for the one-outcome case, never a real
        // task name a user would recognize -- prefixing it added a purely technical label with
        // no content, on top of the real (and, unlike this, legitimately accessibility-motivated
        // -- see `hyperion_workspace::modality`'s own screen-reader role-then-name convention)
        // "status: " announcement already in front of it.
        let contracts: Vec<CapabilityUiContract> = outcomes
            .iter()
            .map(|o| {
                let label = if o.predicate == "generic_goal" {
                    o.detail.clone()
                } else {
                    format!("{}: {}", o.predicate, o.detail)
                };
                contract_for(&o.predicate, &label)
            })
            .collect();

        // A real Context Bundle assembly, scoped to this turn's own real Intent (`intent_id`)
        // but this console's one stable, whole-session `session_id` -- falls back to an empty
        // bundle (still a real, valid `ContextBundle`, just with nothing to bind panels to yet)
        // if assembly itself fails, since a missing context signal shouldn't block rendering the
        // real Agent outcome the user is actually waiting on.
        let scope = Scope {
            intent_id: root.0.to_string(),
            session_id: session_id.to_string(),
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

/// A minimal, real, deterministic utterance-shape recognizer (docs/998-roadmap.md M10) --
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
