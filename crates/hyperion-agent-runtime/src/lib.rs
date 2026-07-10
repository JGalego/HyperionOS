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
//! Deliberately deferred, and why:
//!
//! - **Real sandboxed processes.** There is no `sandbox_class`/container/
//!   micro-VM distinction here — every Agent "process" is just an
//!   [`AgentInstance`] record gated by a [`hyperion_capability::CapabilityToken`],
//!   the same hosted-simulator translation every other crate in this
//!   workspace already uses for a Trust Boundary.
//! - **Real Capability dispatch / Plugin Framework** ([24 — Plugin
//!   Framework](../24-plugin-framework.md), Phase 9) — `invoke()` dispatches
//!   to [`stubs::dispatch`]'s two hand-written stub capabilities, not a real
//!   registry. A capability call can be made to *fail* deterministically
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
//!   serialize the manifest and bound Intent reference only — there is no
//!   real reasoning trace to serialize yet (no real model is driving any
//!   Agent's "next step" — see `hyperion-ai-runtime`'s own mock backend).
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
