//! Hyperion L4 Agent Runtime — Phase 4, first slice.
//!
//! Implements docs/11-agent-runtime.md's core claim made literal: "'agent'
//! is a role a process plays, not a privileged primitive." Every Agent
//! specialization here is a declarative [`AgentManifest`] plus a
//! capability-secured [`AgentInstance`] — one lifecycle state machine, one
//! Capability Broker, one quota/circuit-breaker mechanism, regardless of
//! specialization. There is no per-specialization code path in this crate;
//! specialization lives entirely in *which* capabilities a manifest
//! declares.
//!
//! Real: the full lifecycle state machine (§3.3, narrowed per below), the
//! Capability Broker's three-way grant resolution (§6.1: baseline ->
//! immediate grant, requestable -> consent gate, undeclared -> unconditional
//! deny), a real consecutive-failure circuit breaker (§6.2) that suspends a
//! runaway instance, and checkpoint/resume that revokes open grants rather
//! than carrying them across (§6.3) — "open grants (revoked, not carried
//! across — resume re-requests them)." [`AgentRuntime::invoke`]'s quota
//! gate is the real `hyperion-scheduler` admission algorithm, not a private
//! counter: each invocation submits a real `TaskDescriptor` (one nominal
//! `InferenceTokens` unit, `SchedClass::InteractiveAgent`) to this runtime's
//! own `Scheduler`, runs a real `schedule_epoch`, and only proceeds if the
//! real ledger admits it — releasing the reservation via `complete` the
//! moment dispatch (synchronous, in this simulator) finishes.
//! [`AgentRuntime::resource_headroom`] exposes the real ledger's headroom
//! as queryable proof this round-trips through the real algorithm rather
//! than bypassing it.
//!
//! Per docs/41-implementation-phases.md's own Phase 4 guidance, invocation
//! dispatches to a small, first-party, in-house stub Capability set
//! (`web.search`, `document.draft`) rather than a real Plugin Framework
//! registry — see [`stubs`] and this crate's deferred-scope list below.
//!
//! docs/998-roadmap.md M8 adds exactly one more, real, Capability alongside those two
//! stubs: `assistant.respond` dispatches through a real, caller-supplied
//! [`hyperion_ai_runtime::LocalAiRuntime`] (see [`AgentRuntime::new`]) rather than a stub —
//! real inference behind the exact same Broker/quota/circuit-breaker gate every other
//! Capability call already goes through. This is the one Capability `hyperion-console`'s real
//! undecomposed-goal fallback now calls, closing M8's exit criterion on the path the actually-
//! booted console exercises (`hyperion-console` calls this crate directly; it does not go
//! through `hyperion-api-gateway`/`hyperion-model-router` at all today, which is a separate,
//! already-real wiring path M8 also closed but that the currently-booted console never
//! reaches). See [`runtime::AgentRuntime::dispatch_assistant_respond`]'s own doc comment for
//! why this is a new Capability, not a third case inside an existing stub.
//!
//! docs/998-roadmap.md M10 adds one more real Capability the same way: `web.research`
//! dispatches through a real, caller-supplied [`hyperion_netstack::NetstackHub`] (see
//! [`AgentRuntime::new_with_netstack`]) instead of the stub catch-all — real HTTP/TLS/DNS fetch,
//! real HTML extraction, real merge into the real Knowledge Graph. Unlike `assistant.respond`,
//! this backend is optional (`Option<Arc<NetstackHub>>`, defaulting to none via the unchanged
//! [`AgentRuntime::new`]) since only the real interactive console needs it wired — see
//! [`runtime::AgentRuntime::dispatch_web_research`]'s own doc comment for why this was, again, a
//! real, separate wiring gap (`hyperion-netstack` had zero real callers anywhere in this
//! workspace before this milestone) rather than just a backend swap.
//!
//! docs/998-roadmap.md "Phase 2: cloud providers" adds more real Capabilities the
//! same way — `cloud.openai`/`cloud.anthropic`/`cloud.gemini`/`cloud.groq` all dispatch through
//! [`runtime::AgentRuntime::dispatch_assistant_respond`], the exact same function
//! `assistant.respond` already uses (dispatch itself is backend-agnostic; only *which* real
//! `InferenceBackend` `LocalAiRuntime` was last handed differs). What's genuinely new here is
//! the *gate*: `hyperion-coordination`'s "assistant" manifest declares these four as
//! `requestable_capabilities` (never baseline, unlike `assistant.respond`/`web.research`), so a
//! real `GrantDecision::PendingConsent` round trip (§6.1) — real money, real data leaving the
//! device — stands between a cloud-backed dispatch and ever actually running, where local/mock/
//! self-hosted-engine use stays ungated exactly as before. [`AgentRuntime::grant_capability`] is
//! the other new real piece: a direct grant with no live pending request needed first, which the
//! console uses to make its own "connect my `<provider>`" flow not *also* demand an immediate,
//! redundant re-confirmation the moment a real key is typed in. Deliberately NOT used to carry a
//! grant across a restart, though — the console's own `SecretStore` holding a provider's key
//! proves a real account *exists*, not that this new process has been told it may spend money on
//! it; a fresh boot's first real cloud dispatch still goes through a genuine `PendingConsent`
//! round trip once, so that real, tested path stays actually reachable through a real console
//! session, not just provable in this crate's own isolated tests.
//!
//! **"Do we actually have everything we need to launch a startup already in place?"** No --
//! traced end to end, `document.draft`/`web.search` (the two capabilities
//! `hyperion-coordination`'s own built-in HTN template needs for `business_model`/`branding`/
//! `legal_formation`/`market_research`) dispatched to [`stubs::dispatch`]'s two hand-written
//! canned strings, and even that placeholder text was thrown away by every real caller
//! (`hyperion-coordination::allocate` discarded `InvokeOutcome::Result`'s own value outright) --
//! so a real "launch my startup" run produced zero real content anywhere. Fixed the same way
//! `assistant.respond`/`web.research` were made real: [`runtime::AgentRuntime::
//! dispatch_document_draft`]/[`runtime::AgentRuntime::dispatch_market_research`] now dispatch
//! through the exact same real `LocalAiRuntime` call [`runtime::AgentRuntime::
//! dispatch_assistant_respond`] already established, with a capability-appropriate prompt built
//! from whatever real context the caller sent. `web.search`'s own result is honestly labeled (a
//! `"note"` field) as AI-generated reasoning, not a live web search -- this workspace still has
//! no real search-provider integration, and faking one here would trade one dishonest gap for
//! another. `stubs::dispatch` itself is untouched -- `hyperion-federation`/`hyperion-api-gateway`
//! both call it directly as a deterministic test fixture, unrelated to this fix.
//!
//! Deliberately deferred, and why:
//!
//! - **Real sandboxed processes.** There is no `sandbox_class`/container/
//!   micro-VM distinction here — every Agent "process" is just an
//!   [`AgentInstance`] record gated by a [`hyperion_capability::CapabilityToken`],
//!   the same hosted-simulator translation every other crate in this
//!   workspace already uses for a Trust Boundary.
//! - ~~**Real Capability dispatch / Plugin Framework**~~ ([24 — Plugin
//!   Framework](../24-plugin-framework.md), Phase 9) — `invoke()` dispatches
//!   `assistant.respond`/`web.research`/`document.draft`/`web.search` (plus the three cloud
//!   capabilities) through a real backend apiece; every *other* `capability_ref` is now a real
//!   [`runtime::PluginRegistry::query`] lookup — when a caller wired one in (`plugins: Some(...)`,
//!   e.g. `hyperion-console`'s own `ConsoleSession`) and it names an installed capability backed
//!   by `hyperion_plugin_framework::ImplementationKind::NativeBinary`, the call runs for real
//!   under that registry's Landlock/seccomp sandbox via
//!   `hyperion_plugin_framework::PluginRegistry::invoke_native_binary`. Only a capability *no*
//!   wired registry has installed still falls through to [`stubs::dispatch`]'s catch-all echo
//!   still falls through to [`stubs::dispatch`]'s catch-all echo — never a silent stub for one a
//!   registry actually knows about. A capability call can be made to *fail* deterministically
//!   (pass `{"force_fail": true}` in `args`) specifically so the circuit breaker and
//!   `hyperion-coordination`'s failure-containment logic have something real to react to without
//!   needing a real Capability that can actually fail on its own — both new real dispatch
//!   functions honor this the same way [`stubs::dispatch`] always has.
//! - **Proving the Scheduler gate can actually deny.** `invoke()` already
//!   holds a single global lock across its own entire body, and releases
//!   its Scheduler reservation the instant its (synchronous) dispatch
//!   returns — so under this simulator's current one-call-at-a-time
//!   architecture, no two invocations can ever genuinely overlap, and the
//!   real admission gate above can never be observed denying anything in
//!   a test, only round-tripping. `QuotaState.calls_used_this_window` is
//!   still tracked for observability; `consecutive_failures`/the circuit
//!   breaker are unrelated and untouched by this integration.
//! - **Watchdog heartbeats, real serialized reasoning state.** Checkpoints
//!   serialize the manifest and bound Intent reference only — there is still no real
//!   multi-step reasoning *trace* to serialize: `assistant.respond` (M8, above) is one real
//!   inference call in, one real generated string out, not an Agent that reasons over several
//!   of its own turns and would need that turn history checkpointed.
//! - ~~**User consent UI**~~ — now real for the one case that needed it (docs/998-roadmap.md
//!   "Phase 2: cloud providers"): `hyperion-console` drives a real, synchronous yes/no prompt on
//!   a live `PendingConsent`, then calls [`AgentRuntime::resolve_consent`] with the real answer.
//!   A full [13 — Dynamic UI Runtime](../13-dynamic-ui-runtime.md)-style graphical consent
//!   surface (Phase 5) remains its own, separate, later scope — this is a real text-console
//!   round trip, not that.
//! - ~~**`hyperion-explainability` integration.**~~ (2026-07-18) — now real:
//!   [`AgentRuntime::with_explainability`] wires a real `hyperion_explainability::ExplanationStore`
//!   in, and [`AgentRuntime::invoke`] opens a real Explanation Record around its own dispatch
//!   (`begin` before phase 2, `append_step` + `transition` to `Completed`/`RolledBack` right
//!   after it). The real cycle this bullet used to name — `hyperion-explainability` depended,
//!   transitively via `hyperion-recovery`/`hyperion-privacy`, right back on this crate — was
//!   resolved on `hyperion-explainability`'s own side: it narrowed its `RecoveryPointId`/
//!   `SensitivityClass` fields to local type copies (the same precedent
//!   `hyperion-security::SensitivityHint` already established) instead of depending on those two
//!   crates for nothing more than a `u64` alias and a 4-variant enum. `Option`, not automatic: a
//!   caller that already wraps `invoke()` externally with its own Explanation Record
//!   (`hyperion-coordination`, `hyperion-federation`) should not also wire this in, or every real
//!   dispatch would be recorded twice. `hyperion-console` — which had no Explanation Record
//!   wiring at all before this — is the new real, direct beneficiary.

mod broker;
mod runtime;
mod stubs;
mod types;

pub use runtime::{AgentError, AgentRuntime};
pub use stubs::dispatch as dispatch_stub_capability;
pub use types::{
    AgentCheckpoint, AgentInstance, AgentManifest, AuditEntry, CapabilityGrant, InvokeOutcome,
    LifecycleState, QuotaState, TrustTier,
};
