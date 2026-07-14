use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use hyperion_ai_runtime::{CapabilityContract, InferenceRequest, LocalAiRuntime, ModelClass};
use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_netstack::{FreshnessPolicy, NetstackHub, WebResolutionRequest};
use hyperion_plugin_framework::PluginRegistry;
use hyperion_scheduler::{
    AgentId, IntentId, ResourceDimension, ResourceLedger, ResourceVector, SchedClass, Scheduler,
    TaskDescriptor, TaskId,
};

use crate::broker::{self, GrantDecision};
use crate::stubs;
use crate::types::{
    AgentCheckpoint, AgentInstance, AgentManifest, AuditEntry, CapabilityGrant, InvokeOutcome,
    LifecycleState, QuotaState,
};

/// The one Capability this crate dispatches to a real backend instead of [`stubs::dispatch`] --
/// see [`AgentRuntime::dispatch_assistant_respond`]'s own doc comment for why this is a
/// genuinely new Capability rather than a third stub, and PRODUCTION_BOOT_PROMPT.md's M8 note
/// for why the fallback path needed a real one at all.
const ASSISTANT_RESPOND_CAPABILITY: &str = "assistant.respond";
/// PRODUCTION_BOOT_PROMPT.md M10's real Capability alongside `assistant.respond` -- see
/// [`AgentRuntime::dispatch_web_research`]'s own doc comment.
const WEB_RESEARCH_CAPABILITY: &str = "web.research";
/// PRODUCTION_BOOT_PROMPT.md "Phase 2: cloud providers": the real, requestable (never
/// baseline -- see `hyperion-coordination::catalog::default_manifests`) Capabilities a real
/// cloud dispatch is gated behind. Each routes to the exact same
/// [`AgentRuntime::dispatch_assistant_respond`] as the baseline `assistant.respond` case below --
/// dispatch itself is backend-agnostic (whatever `LocalAiRuntime`'s currently-active backend is),
/// so only the *gate* differs between local and cloud use, not the dispatch function. The console
/// picks which of these literal strings to invoke under based on its own currently-active
/// backend (`hyperion_console::session::BackendKind::capability_ref`) -- these stay private
/// constants here, exactly as `ASSISTANT_RESPOND_CAPABILITY` already does, with the console
/// hardcoding the matching literals rather than importing them. `CLOUD_GROQ_CAPABILITY` joined
/// the other three later (Groq support) -- it's a distinct, real, paid, hosted cloud provider
/// (Groq's own LPU-hosted API), gated exactly like the rest, even though its wire protocol
/// happens to be OpenAI-compatible (that's an implementation detail of the console's own
/// `try_connect_groq`, not a trust-boundary distinction).
const CLOUD_OPENAI_CAPABILITY: &str = "cloud.openai";
const CLOUD_ANTHROPIC_CAPABILITY: &str = "cloud.anthropic";
const CLOUD_GEMINI_CAPABILITY: &str = "cloud.gemini";
const CLOUD_GROQ_CAPABILITY: &str = "cloud.groq";
/// Real now, alongside `assistant.respond`/`web.research` above -- see
/// [`AgentRuntime::dispatch_document_draft`]'s own doc comment for what changed and why.
const DOCUMENT_DRAFT_CAPABILITY: &str = "document.draft";
/// See [`AgentRuntime::dispatch_market_research`]'s own doc comment.
const WEB_SEARCH_CAPABILITY: &str = "web.search";

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs()
}

/// docs/11 §6.2: "a circuit breaker trips after N consecutive Capability
/// failures within one window."
const CIRCUIT_BREAKER_THRESHOLD: u32 = 3;
const DEFAULT_QUOTA: u32 = 100;

/// [`AgentRuntime::prepare_invoke`]'s own result -- see [`AgentRuntime::invoke`]'s doc comment
/// for why this three-phase split exists at all.
enum PreparedInvoke {
    /// Already a final `InvokeOutcome` -- nothing left to dispatch (denied, pending consent, or
    /// quota exceeded).
    Resolved(InvokeOutcome),
    /// Admitted; carries the real Scheduler ticket [`AgentRuntime::invoke`]'s own phase 3 must
    /// `complete` once the real dispatch (phase 2, no lock held) returns.
    Proceed(TaskId),
}

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("no such agent instance")]
    NotFound,
    #[error("invalid state transition: {0}")]
    InvalidState(String),
}

/// docs/11 — Agent Runtime. See this crate's doc comment for what's
/// deferred.
pub struct AgentRuntime {
    instances: Mutex<HashMap<u64, AgentInstance>>,
    checkpoints: Mutex<HashMap<u64, AgentCheckpoint>>,
    next_id: AtomicU64,
    /// docs/04's real unified Scheduler, backing [`Self::invoke`]'s quota
    /// gate — see this crate's doc comment on why admission is delegated
    /// here instead of `QuotaState`'s own private counter.
    scheduler: Mutex<Scheduler>,
    next_task_id: AtomicU64,
    /// Real backend for [`ASSISTANT_RESPOND_CAPABILITY`] — see
    /// [`Self::dispatch_assistant_respond`]. Caller-supplied (not built
    /// internally) so the caller controls which [`hyperion_ai_runtime::InferenceBackend`]
    /// and which registered [`hyperion_ai_runtime::ModelDescriptor`]s back it: a real
    /// `CandleBackend` on a booted image, `MockBackend` in every host-side test, the same
    /// "swap the backend, not the call site" principle `hyperion-ai-runtime` and
    /// `hyperion-api-gateway` already established for M8.
    ai_runtime: Arc<LocalAiRuntime>,
    /// Real backend for [`WEB_RESEARCH_CAPABILITY`] — see [`Self::dispatch_web_research`].
    /// `Option`, not a required constructor parameter like `ai_runtime`: unlike inference (every
    /// real caller of this runtime wants `assistant.respond` available), only the one real
    /// interactive console instance needs real network access wired up. Threading a
    /// `NetstackHub` (itself needing a real `Arc<KnowledgeGraph>`) through this struct's
    /// constructor would force all 13 existing call sites -- most of which have no Knowledge
    /// Graph of their own at all (`hyperion-federation`'s per-device instances, most of this
    /// crate's own tests) -- to acquire one just to satisfy a parameter they'd never use. See
    /// [`Self::new_with_netstack`].
    netstack: Option<Arc<NetstackHub>>,
    /// Real backend for any `capability_ref` this crate doesn't otherwise recognize -- see
    /// [`Self::invoke`]'s dispatch chain. `Option`, same reasoning as [`Self::netstack`]: most of
    /// this crate's own 13+ existing call sites have no installed plugins at all and shouldn't
    /// need to acquire an empty [`PluginRegistry`] just to satisfy a required parameter. See
    /// [`Self::new_with_netstack_and_plugins`].
    plugins: Option<Arc<PluginRegistry>>,
}

impl AgentRuntime {
    pub fn new(ai_runtime: Arc<LocalAiRuntime>) -> Self {
        Self::new_with_netstack_and_plugins(ai_runtime, None, None)
    }

    /// As [`Self::new`], additionally wiring a real [`NetstackHub`] so `web.research` dispatches
    /// to real network fetch/extraction/Knowledge-Graph-merge instead of falling through to
    /// [`stubs::dispatch`]'s catch-all echo (PRODUCTION_BOOT_PROMPT.md M10).
    pub fn new_with_netstack(
        ai_runtime: Arc<LocalAiRuntime>,
        netstack: Option<Arc<NetstackHub>>,
    ) -> Self {
        Self::new_with_netstack_and_plugins(ai_runtime, netstack, None)
    }

    /// As [`Self::new_with_netstack`], additionally wiring a real [`PluginRegistry`] so an
    /// unrecognized `capability_ref` with an installed, real `NativeBinary` implementation
    /// dispatches to it for real (AUTONOMY_ROADMAP.md's Slice 1), instead of falling through to
    /// [`stubs::dispatch`]'s catch-all echo.
    pub fn new_with_netstack_and_plugins(
        ai_runtime: Arc<LocalAiRuntime>,
        netstack: Option<Arc<NetstackHub>>,
        plugins: Option<Arc<PluginRegistry>>,
    ) -> Self {
        let mut scheduler = Scheduler::new();
        // One nominal dimension stands in for "a Capability invocation's
        // resource footprint" — `DEFAULT_QUOTA` reused as the ledger's
        // capacity keeps this the same number `QuotaState` always used,
        // just enforced by the real admission algorithm instead of a
        // private counter.
        scheduler.register_resource_provider(ResourceLedger::new(
            ResourceDimension::InferenceTokens,
            DEFAULT_QUOTA,
            0,
        ));
        AgentRuntime {
            instances: Mutex::new(HashMap::new()),
            checkpoints: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            scheduler: Mutex::new(scheduler),
            next_task_id: AtomicU64::new(1),
            ai_runtime,
            netstack,
            plugins,
        }
    }

    /// Real headroom remaining on the Scheduler's single
    /// `InferenceTokens` ledger this runtime's Capability invocations
    /// draw from — queryable proof that [`Self::invoke`] round-trips
    /// through the real admission algorithm rather than a private
    /// counter: it reads `DEFAULT_QUOTA` before any call and after every
    /// call, since each invocation's resource request is released the
    /// moment its (synchronous, in this simulator) dispatch finishes.
    pub fn resource_headroom(&self) -> u32 {
        self.scheduler
            .lock()
            .unwrap()
            .query_ledger(ResourceDimension::InferenceTokens)
            .map(|l| l.headroom(false))
            .unwrap_or(0)
    }

    fn require(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        rights: RightsMask,
    ) -> Result<(), AgentError> {
        monitor
            .check_rights_ok_result(token, rights)
            .map_err(|_| AgentError::Unauthorized)
    }

    fn audit(instance: &mut AgentInstance, kind: &str, detail: impl Into<String>) {
        instance.audit_log.push(AuditEntry {
            timestamp: now(),
            kind: kind.to_string(),
            detail: detail.into(),
        });
    }

    /// `AgentRuntime.spawn` fused with `bind` — docs/11 §7's signature
    /// already takes `intent_ref`/`context_bundle_ref` at spawn time, so
    /// this crate does not model a separate unbound `spawning` state
    /// observable to callers.
    pub fn spawn(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        manifest: AgentManifest,
        bound_intent: Option<u64>,
    ) -> Result<u64, AgentError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let instance_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut instance = AgentInstance {
            instance_id,
            manifest,
            state: LifecycleState::Bound,
            bound_intent,
            grants: Vec::new(),
            quota: QuotaState::new(DEFAULT_QUOTA),
            pending_consent: None,
            audit_log: Vec::new(),
        };
        Self::audit(&mut instance, "bound", format!("intent={bound_intent:?}"));
        self.instances.lock().unwrap().insert(instance_id, instance);
        Ok(instance_id)
    }

    /// docs/11 §7's `invoke` — routed through the Broker (§6.1) and quota/
    /// circuit breaker (§6.2), then dispatched to a real or stub Capability.
    ///
    /// Deliberately three phases, not one lock held start to finish: a real, previously-shipped
    /// bottleneck this split fixes -- `hyperion-coordination`'s own `allocate` dispatches every
    /// *ready* task in one tick (e.g. `business_model` and `branding`, both real HTN-template
    /// siblings that genuinely don't depend on each other), and since this crate's own allocator
    /// reuses one Agent instance per specialization rather than spawning one per task (proven by
    /// `hyperion-coordination`'s own "one research + one writer instance, reused across tasks"
    /// test), two independent tasks routinely land on the *same* `instance_id`. Holding
    /// `self.instances`' single global lock across the real capability dispatch -- which can now
    /// be a real, slow network call to a real cloud model (see `dispatch_document_draft`/
    /// `dispatch_market_research`'s own doc comments) -- would serialize every invocation in the
    /// whole runtime behind it, defeating any concurrent dispatch a caller attempts no matter how
    /// many real OS threads it spawns. [`Self::prepare_invoke`] (phase 1, locked) is the only part
    /// that must serialize: lifecycle/Broker/Scheduler checks and the bookkeeping state
    /// transition. The dispatch itself (phase 2) runs with **no lock held at all**. Phase 3
    /// re-acquires the lock only to record the real outcome.
    pub fn invoke(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        instance_id: u64,
        capability_ref: &str,
        args: serde_json::Value,
    ) -> Result<InvokeOutcome, AgentError> {
        self.require(monitor, token, RightsMask::EXEC)?;

        let ticket = match self.prepare_invoke(monitor, token, instance_id, capability_ref)? {
            PreparedInvoke::Resolved(outcome) => return Ok(outcome),
            PreparedInvoke::Proceed(ticket) => ticket,
        };

        // Phase 2: the real capability dispatch, with no lock held -- see this function's own
        // doc comment.
        let dispatch_result = if [
            ASSISTANT_RESPOND_CAPABILITY,
            CLOUD_OPENAI_CAPABILITY,
            CLOUD_ANTHROPIC_CAPABILITY,
            CLOUD_GEMINI_CAPABILITY,
            CLOUD_GROQ_CAPABILITY,
        ]
        .contains(&capability_ref)
        {
            self.dispatch_assistant_respond(monitor, token, &args)
        } else if capability_ref == WEB_RESEARCH_CAPABILITY && self.netstack.is_some() {
            self.dispatch_web_research(monitor, token, instance_id, &args)
        } else if capability_ref == DOCUMENT_DRAFT_CAPABILITY {
            self.dispatch_document_draft(monitor, token, &args)
        } else if capability_ref == WEB_SEARCH_CAPABILITY {
            self.dispatch_market_research(monitor, token, &args)
        } else if let Some(plugins) = self.plugins.as_ref().filter(|p| {
            p.query(capability_ref).is_some_and(|entry| {
                entry
                    .implementations
                    .iter()
                    .any(|i| i.native_binary.is_some())
            })
        }) {
            plugins
                .invoke_native_binary(capability_ref, args.clone())
                .map_err(|e| e.to_string())
        } else {
            stubs::dispatch(capability_ref, &args)
        };

        // Phase 3: re-acquire the lock only to record the real outcome.
        let _ = self.scheduler.lock().unwrap().complete(ticket);
        let mut instances = self.instances.lock().unwrap();
        let instance = instances
            .get_mut(&instance_id)
            .ok_or(AgentError::NotFound)?;
        match dispatch_result {
            Ok(result) => {
                instance.quota.consecutive_failures = 0;
                Self::audit(instance, "invoked", capability_ref);
                Ok(InvokeOutcome::Result(result))
            }
            Err(reason) => {
                instance.quota.consecutive_failures += 1;
                Self::audit(
                    instance,
                    "capability_failed",
                    format!("{capability_ref}: {reason}"),
                );
                if instance.quota.consecutive_failures >= CIRCUIT_BREAKER_THRESHOLD {
                    instance.state = LifecycleState::Suspended;
                    Self::audit(instance, "suspended_runaway", capability_ref);
                }
                Ok(InvokeOutcome::Failed(reason))
            }
        }
    }

    /// [`Self::invoke`]'s phase 1 -- every check that must serialize against this instance's own
    /// bookkeeping, all of it fast, in-memory work: lifecycle state, the Broker's grant decision,
    /// and the real Scheduler admission gate (docs/04, replacing a private
    /// `QuotaState.has_headroom()` counter that never touched the rest of the system's real
    /// resource model). `PreparedInvoke::Resolved` covers every case that's already a final
    /// `InvokeOutcome` before any real dispatch would even be attempted (denied, pending consent,
    /// quota exceeded); `PreparedInvoke::Proceed` carries the admitted `TaskId` ticket `invoke`'s
    /// own phase 3 needs to `complete`.
    fn prepare_invoke(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        instance_id: u64,
        capability_ref: &str,
    ) -> Result<PreparedInvoke, AgentError> {
        let mut instances = self.instances.lock().unwrap();
        let instance = instances
            .get_mut(&instance_id)
            .ok_or(AgentError::NotFound)?;

        match instance.state {
            LifecycleState::Terminated
            | LifecycleState::Completed
            | LifecycleState::Failed
            | LifecycleState::Suspended => {
                return Err(AgentError::InvalidState(format!(
                    "cannot invoke while {:?}",
                    instance.state
                )));
            }
            _ => {}
        }

        match broker::resolve_grant(instance, capability_ref) {
            GrantDecision::Denied => {
                Self::audit(instance, "denied", capability_ref);
                return Ok(PreparedInvoke::Resolved(InvokeOutcome::Denied));
            }
            GrantDecision::PendingConsent => {
                instance.state = LifecycleState::WaitingOnCapability;
                instance.pending_consent = Some(capability_ref.to_string());
                Self::audit(instance, "pending_consent", capability_ref);
                return Ok(PreparedInvoke::Resolved(InvokeOutcome::PendingConsent));
            }
            GrantDecision::Granted => {}
        }

        let task_id = TaskId(self.next_task_id.fetch_add(1, Ordering::Relaxed));
        let ticket = self
            .scheduler
            .lock()
            .unwrap()
            .submit_task(
                monitor,
                TaskDescriptor {
                    id: task_id,
                    owner_intent: IntentId(instance.bound_intent.unwrap_or(0)),
                    owner_agent: Some(AgentId(instance_id)),
                    class: SchedClass::InteractiveAgent,
                    deadline: None,
                    priority_weight: 1.0,
                    request: ResourceVector {
                        inference_tokens_per_sec: 1,
                        ..Default::default()
                    },
                    cap_token: token.clone(),
                },
            )
            .map_err(|_| AgentError::Unauthorized)?;
        let admitted = self
            .scheduler
            .lock()
            .unwrap()
            .schedule_epoch()
            .into_iter()
            .find(|r| r.ticket == ticket)
            .map(|r| r.admitted)
            .unwrap_or(false);
        if !admitted {
            let _ = self.scheduler.lock().unwrap().cancel(ticket);
            Self::audit(instance, "quota_exceeded", capability_ref);
            return Ok(PreparedInvoke::Resolved(InvokeOutcome::QuotaExceeded));
        }

        instance.state = LifecycleState::Executing;
        instance.quota.calls_used_this_window += 1;
        Ok(PreparedInvoke::Proceed(ticket))
    }

    /// The one Capability [`Self::invoke`] dispatches to a real backend rather than
    /// [`stubs::dispatch`]'s hand-written stand-ins -- PRODUCTION_BOOT_PROMPT.md M8's exit
    /// criterion made real on the path the actually-booted console exercises, not only in
    /// `hyperion-api-gateway`/`hyperion-model-router` (which the booted console's own real
    /// call path -- `hyperion-console` -> [`Self::invoke`] -- never reaches; see this crate's
    /// doc comment). Deliberately its own named Capability rather than a new case inside
    /// `web.search`/`document.draft`: those two remain what docs/41 Phase 4 always meant them
    /// to be -- stand-ins for a future *real network fetch* / *real document generation*
    /// (M10's and a later Capability's job respectively) -- while this one already *is* the
    /// real thing it names, gated only by whichever `InferenceBackend` the caller's
    /// `LocalAiRuntime` was constructed with.
    ///
    /// Every other real mechanism in [`Self::invoke`] (Broker grant, quota, circuit breaker)
    /// applies identically before this is ever reached -- only the dispatch step itself
    /// branches, exactly as `hyperion-api-gateway::ApiGateway::dispatch_one` does for the same
    /// reason.
    fn dispatch_assistant_respond(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        self.run_inference(monitor, token, prompt, "text")
    }

    /// The real shape every inference-backed dispatch function in this crate shares: a bounded
    /// latency budget generous enough for a real tiny CPU-only model
    /// (`hyperion_ai_runtime::LocalAiRuntime::estimate`'s own feasibility check assumes a
    /// ~100-token response -- `hyperion-console`'s own registered `expected_tokens_per_sec: 10.0`
    /// needs this budget comfortably above the resulting ~10s estimate, or every real call would
    /// be rejected as infeasible before ever reaching the backend), and the real, currently-active
    /// `InferenceBackend` (`ai_runtime`) rather than a hand-written stand-in. `result_key` is the
    /// JSON key the real generated text is returned under, so each capability keeps its own
    /// caller-facing shape (`assistant.respond`'s `"text"`, `document.draft`'s `"draft"`, ...)
    /// without duplicating this call three times over.
    fn run_inference(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        prompt: String,
        result_key: &str,
    ) -> Result<serde_json::Value, String> {
        let contract = CapabilityContract {
            latency_budget_ms: 15_000,
            always_on: false,
        };
        let request = InferenceRequest { prompt };
        self.ai_runtime
            .infer(monitor, token, ModelClass::Slm, &contract, &request)
            .map(|result| serde_json::json!({ result_key: result.text }))
            .map_err(|e| e.to_string())
    }

    /// A real bug this crate's own doc comment now records: `document.draft` used to be one of
    /// [`stubs::dispatch`]'s two hand-written stand-ins (a fixed `"Stub draft document about
    /// '{topic}'."` string) -- and even that canned text was thrown away by every real caller
    /// (`hyperion-coordination::allocate` discarded `InvokeOutcome::Result`'s own value outright).
    /// Real now, via the exact same "run it through whichever `InferenceBackend` is currently
    /// active" shape [`Self::dispatch_assistant_respond`] already established -- the real backend
    /// was always one function call away, just never wired up for this capability.
    ///
    /// [`stubs::dispatch`] itself is deliberately untouched: `hyperion-federation` and
    /// `hyperion-api-gateway` both call it *directly*, bypassing this crate's own `invoke`
    /// entirely, as a deterministic fixture for their own, unrelated tests (placement scoring,
    /// router fallback) -- changing its behavior out from under them would be a real regression
    /// for no benefit. This dispatches through a parallel, real path instead, reached only via
    /// [`Self::invoke`]'s own dispatch match below.
    fn dispatch_document_draft(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Self::check_force_fail(DOCUMENT_DRAFT_CAPABILITY, args)?;
        let subject = Self::extract_subject(args);
        let mut prompt = format!("Draft a concise, practical {subject}.");
        Self::append_extra_context(&mut prompt, args);
        self.run_inference(monitor, token, prompt, "draft")
    }

    /// See [`Self::dispatch_document_draft`]'s own doc comment -- same fix, same reasoning, for
    /// `web.search`. Deliberately honest about what this still is *not*: a real generated summary
    /// from whichever `InferenceBackend` is active, not a live query against a real search engine
    /// -- this workspace has no search-provider integration at all (a real, separate, future
    /// feature akin to PRODUCTION_BOOT_PROMPT.md's cloud-provider phase, not something to fake
    /// here). The returned `"note"` field carries that caveat through to anything that renders
    /// this result, so nothing downstream can mistake it for a verified web search.
    fn dispatch_market_research(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Self::check_force_fail(WEB_SEARCH_CAPABILITY, args)?;
        let subject = Self::extract_subject(args);
        let mut prompt = format!(
            "Provide a concise research summary about {subject}. Be clear that this is your \
             own reasoning, not verified live information."
        );
        Self::append_extra_context(&mut prompt, args);
        let result = self.run_inference(monitor, token, prompt, "text")?;
        let text = result
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        Ok(serde_json::json!({
            "results": [text],
            "note": "AI-generated research notes, not a live web search",
        }))
    }

    /// `args = {"force_fail": true}` deterministically fails any capability -- see
    /// [`stubs::dispatch`]'s own doc comment for why that test seam exists
    /// (`hyperion-coordination`'s own retry/escalation tests inject it via
    /// [`crate::AgentRuntime`]'s real dispatch, not only through the stub). Both new real
    /// dispatch functions above need to honor it too, or those tests would silently start
    /// exercising a real inference call instead of the deliberate failure they ask for.
    fn check_force_fail(capability_ref: &str, args: &serde_json::Value) -> Result<(), String> {
        if args
            .get("force_fail")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return Err(format!(
                "capability '{capability_ref}' was forced to fail for testing"
            ));
        }
        Ok(())
    }

    /// Whatever subject text is present in `args`, under any of these real key names -- a caller
    /// built for the old stub (`"query"`/`"topic"`) and `hyperion-coordination`'s own richer args
    /// (`"task"`/`"goal"`) both produce a real, meaningful prompt without either caller needing to
    /// change what it sends.
    fn extract_subject(args: &serde_json::Value) -> String {
        let parts: Vec<&str> = ["query", "topic", "task", "goal"]
            .iter()
            .filter_map(|key| args.get(*key).and_then(|v| v.as_str()))
            .filter(|s| !s.is_empty())
            .collect();
        if parts.is_empty() {
            "the requested topic".to_string()
        } else {
            parts.join(" -- ")
        }
    }

    /// Appends real, user-supplied steering text to `prompt`, when the caller sent one -- the
    /// real "redo this with more information" verb `hyperion-coordination::CoordinationSession::
    /// amend_task` sets on a task before its next real dispatch. Absent for every task's first
    /// (never-redone) dispatch, so this is purely additive: nothing changes for a caller that
    /// never sends `"extra_context"` at all.
    fn append_extra_context(prompt: &mut String, args: &serde_json::Value) {
        if let Some(extra) = args
            .get("extra_context")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            prompt.push_str(&format!(" Additional instructions from the user: {extra}"));
        }
    }

    /// PRODUCTION_BOOT_PROMPT.md M10's real Capability: dispatches to a real, caller-supplied
    /// [`NetstackHub`] instead of [`stubs::dispatch`]'s catch-all echo -- the real fix for the
    /// same two-link dead chain M8 found for `assistant.respond`, one milestone later:
    /// `hyperion-netstack` had zero real (non-test) callers anywhere in this workspace, and
    /// `hyperion-compat` (the one crate that *did* call it for real) was itself never constructed
    /// outside its own tests. Reachable here only when [`Self::netstack`] is `Some` (see that
    /// field's own doc comment on why it's optional, unlike `ai_runtime`) -- when it's `None`,
    /// `"web.research"` falls through to `stubs::dispatch`'s generic `{"echo": ..., "args": ...}`
    /// case, same as any other undeclared capability, rather than a hard failure.
    fn dispatch_web_research(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        instance_id: u64,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let netstack = self
            .netstack
            .as_ref()
            .ok_or_else(|| "no real network backend is wired up for this runtime".to_string())?;
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let request = WebResolutionRequest {
            origin: url,
            agent_id: instance_id,
            purpose: "agent-initiated web research".to_string(),
            freshness: FreshnessPolicy::UseCache,
            depth: 0,
        };
        netstack
            .web_research(monitor, token, &request, now())
            .map(|result| {
                serde_json::json!({
                    "object_id": result.object_id.0,
                    "stale": result.stale,
                    "needs_review": result.needs_review,
                })
            })
            .map_err(|e| e.to_string())
    }

    /// docs/11 §6.1: the consent round trip's resolution — see this
    /// crate's doc comment on the deferred real UI prompt.
    pub fn resolve_consent(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        instance_id: u64,
        approved: bool,
    ) -> Result<(), AgentError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let mut instances = self.instances.lock().unwrap();
        let instance = instances
            .get_mut(&instance_id)
            .ok_or(AgentError::NotFound)?;
        let Some(capability_ref) = instance.pending_consent.take() else {
            return Err(AgentError::InvalidState(
                "no pending consent request".to_string(),
            ));
        };

        if approved {
            instance.grants.push(CapabilityGrant {
                capability_ref: capability_ref.clone(),
                scope: Vec::new(),
                granted_at: now(),
            });
            Self::audit(instance, "consent_granted", capability_ref);
        } else {
            Self::audit(instance, "consent_denied", capability_ref);
        }
        instance.state = LifecycleState::Executing;
        Ok(())
    }

    /// PRODUCTION_BOOT_PROMPT.md "Phase 2: cloud providers": grants `capability_ref` directly,
    /// with no live [`GrantDecision::PendingConsent`] required first -- unlike
    /// [`Self::resolve_consent`] (which only ever resolves a request `invoke` itself just made,
    /// and errors if there isn't one), this is for seeding an *already-consented* capability at
    /// session startup. The console calls this once per provider already present in its own
    /// `SecretStore`: the user only ever gets a secret into that store via an explicit "connect
    /// my `<provider>`" utterance, so its mere presence already proves consent -- re-prompting
    /// for it on every restart would be real, unnecessary friction, not real caution.
    pub fn grant_capability(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        instance_id: u64,
        capability_ref: &str,
    ) -> Result<(), AgentError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let mut instances = self.instances.lock().unwrap();
        let instance = instances
            .get_mut(&instance_id)
            .ok_or(AgentError::NotFound)?;
        instance.grants.push(CapabilityGrant {
            capability_ref: capability_ref.to_string(),
            scope: Vec::new(),
            granted_at: now(),
        });
        Self::audit(instance, "consent_granted_direct", capability_ref);
        Ok(())
    }

    /// docs/11 §6.3: serializes the manifest and bound Intent reference,
    /// revokes open grants (never carried across — resume re-requests
    /// them), and tears down.
    pub fn checkpoint(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        instance_id: u64,
    ) -> Result<u64, AgentError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let mut instances = self.instances.lock().unwrap();
        let instance = instances
            .get_mut(&instance_id)
            .ok_or(AgentError::NotFound)?;

        let checkpoint_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let checkpoint = AgentCheckpoint {
            checkpoint_id,
            instance_id,
            manifest: instance.manifest.clone(),
            bound_intent: instance.bound_intent,
            created_at: now(),
        };
        instance.grants.clear();
        instance.state = LifecycleState::Checkpointed;
        Self::audit(instance, "checkpointed", checkpoint_id.to_string());
        self.checkpoints
            .lock()
            .unwrap()
            .insert(checkpoint_id, checkpoint);
        Ok(checkpoint_id)
    }

    /// docs/11 §6.3: re-binds the same Intent, rehydrates state, returns to
    /// `executing` — grants must be re-requested, per checkpoint's revoke.
    pub fn resume(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        checkpoint_id: u64,
    ) -> Result<u64, AgentError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let checkpoint = self
            .checkpoints
            .lock()
            .unwrap()
            .get(&checkpoint_id)
            .cloned()
            .ok_or(AgentError::NotFound)?;

        let mut instances = self.instances.lock().unwrap();
        let instance = instances
            .get_mut(&checkpoint.instance_id)
            .ok_or(AgentError::NotFound)?;
        if instance.state != LifecycleState::Checkpointed {
            return Err(AgentError::InvalidState(format!(
                "cannot resume from {:?}",
                instance.state
            )));
        }
        instance.state = LifecycleState::Executing;
        Self::audit(instance, "resumed", checkpoint_id.to_string());
        Ok(checkpoint.instance_id)
    }

    /// Exposes a checkpoint's contents (manifest, bound Intent reference)
    /// so a caller orchestrating *across* two `AgentRuntime` instances —
    /// `hyperion-federation`'s cross-device migration is the motivating
    /// case — can transfer it, since [`Self::resume`] only ever continues
    /// an instance record within this same runtime.
    pub fn get_checkpoint(&self, checkpoint_id: u64) -> Option<AgentCheckpoint> {
        self.checkpoints
            .lock()
            .unwrap()
            .get(&checkpoint_id)
            .cloned()
    }

    pub fn terminate(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        instance_id: u64,
        reason: &str,
    ) -> Result<(), AgentError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let mut instances = self.instances.lock().unwrap();
        let instance = instances
            .get_mut(&instance_id)
            .ok_or(AgentError::NotFound)?;
        instance.state = LifecycleState::Terminated;
        Self::audit(instance, "terminated", reason);
        Ok(())
    }

    pub fn mark_completed(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        instance_id: u64,
    ) -> Result<(), AgentError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let mut instances = self.instances.lock().unwrap();
        let instance = instances
            .get_mut(&instance_id)
            .ok_or(AgentError::NotFound)?;
        instance.state = LifecycleState::Completed;
        Self::audit(instance, "completed", "");
        Ok(())
    }

    pub fn describe(&self, instance_id: u64) -> Option<AgentInstance> {
        self.instances.lock().unwrap().get(&instance_id).cloned()
    }

    pub fn state_of(&self, instance_id: u64) -> Option<LifecycleState> {
        self.instances
            .lock()
            .unwrap()
            .get(&instance_id)
            .map(|i| i.state)
    }
}
