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
//! deny), a token-bucket quota with a real consecutive-failure circuit
//! breaker (§6.2) that suspends a runaway instance, and checkpoint/resume
//! that revokes open grants rather than carrying them across (§6.3) — "open
//! grants (revoked, not carried across — resume re-requests them)."
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
//! - **Real Scheduler quota integration** ([04 — Scheduler](../04-scheduler.md),
//!   already real in this workspace) — `QuotaState` here is a self-contained
//!   token bucket, not wired into `hyperion-scheduler`'s `ResourceLedger`/
//!   admission model. Real integration is a legitimate next step but a
//!   separate slice from this one.
//! - **Watchdog heartbeats, real serialized reasoning state.** Checkpoints
//!   serialize the manifest and bound Intent reference only — there is no
//!   real reasoning trace to serialize yet (no real model is driving any
//!   Agent's "next step" — see `hyperion-ai-runtime`'s own mock backend).
//! - **User consent UI** ([13 — Dynamic UI Runtime](../13-dynamic-ui-runtime.md),
//!   Phase 5) — [`AgentRuntime::resolve_consent`] is a direct, caller-driven
//!   API standing in for a real consent prompt round-trip.

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
