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
//! PRODUCTION_BOOT_PROMPT.md M8 adds exactly one more, real, Capability alongside those two
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
//! PRODUCTION_BOOT_PROMPT.md M10 adds one more real Capability the same way: `web.research`
//! dispatches through a real, caller-supplied [`hyperion_netstack::NetstackHub`] (see
//! [`AgentRuntime::new_with_netstack`]) instead of the stub catch-all — real HTTP/TLS/DNS fetch,
//! real HTML extraction, real merge into the real Knowledge Graph. Unlike `assistant.respond`,
//! this backend is optional (`Option<Arc<NetstackHub>>`, defaulting to none via the unchanged
//! [`AgentRuntime::new`]) since only the real interactive console needs it wired — see
//! [`runtime::AgentRuntime::dispatch_web_research`]'s own doc comment for why this was, again, a
//! real, separate wiring gap (`hyperion-netstack` had zero real callers anywhere in this
//! workspace before this milestone) rather than just a backend swap.
//!
//! Deliberately deferred, and why:
//!
//! - **Real sandboxed processes.** There is no `sandbox_class`/container/
//!   micro-VM distinction here — every Agent "process" is just an
//!   [`AgentInstance`] record gated by a [`hyperion_capability::CapabilityToken`],
//!   the same hosted-simulator translation every other crate in this
//!   workspace already uses for a Trust Boundary.
//! - **Real Capability dispatch / Plugin Framework** ([24 — Plugin
//!   Framework](../24-plugin-framework.md), Phase 9) — `invoke()` dispatches
//!   `web.search`/`document.draft` to [`stubs::dispatch`]'s two hand-written
//!   stand-ins, not a real registry (`assistant.respond` is the one exception — see the M8
//!   note above — and even it is a single fixed Capability this crate special-cases, not a
//!   registry entry). A capability call can be made to *fail* deterministically
//!   (pass `{"force_fail": true}` in `args`) specifically so the circuit
//!   breaker and `hyperion-coordination`'s failure-containment logic (next
//!   in this phase) have something real to react to without needing a real
//!   Capability that can actually fail on its own.
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
//! - **User consent UI** ([13 — Dynamic UI Runtime](../13-dynamic-ui-runtime.md),
//!   Phase 5) — [`AgentRuntime::resolve_consent`] is a direct, caller-driven
//!   API standing in for a real consent prompt round-trip.
//! - **`hyperion-explainability` integration.** This crate cannot depend
//!   on `hyperion-explainability` directly — `hyperion-explainability`
//!   depends on `hyperion-recovery`, which itself depends on this crate
//!   (for crash-recovery reconciliation against in-flight Agent state),
//!   so a direct dependency here would be a real cycle. Explanation
//!   Record wiring for a dispatched Capability call belongs one layer up,
//!   in whichever crate calls [`AgentRuntime::invoke`] (`hyperion-coordination`,
//!   `hyperion-federation`) and isn't itself downstream of `hyperion-recovery`.

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
