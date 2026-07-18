//! Hyperion unified multi-resource scheduler.
//!
//! Implements docs/04-scheduler.md's admission control and Weighted-EDF +
//! Dominant-Resource-Fair dispatch across CPU/RAM/GPU/VRAM/storage/network/
//! inference-tokens/context-window/battery as one multi-dimensional
//! allocation problem, gated by `hyperion-capability` tokens exactly like
//! every other Phase 1 resource.
//!
//! - ~~**Distributed offload.**~~ (2026-07-18) — now real, via dependency injection rather than a
//!   direct `hyperion-federation` dependency (`hyperion-federation` already depends on *this*
//!   crate for `ResourceVector`/`ResourceDimension`, so the reverse would be a hard Cargo cycle):
//!   [`Scheduler::with_offload_trigger`] wires in any [`scheduler::OffloadTrigger`] implementation,
//!   and [`Scheduler::schedule_epoch`]'s non-admit branch calls it for a
//!   `SchedClass::BatchDistributable` task that also failed model-tier degradation, before finally
//!   falling back to aging and requeuing. `hyperion_federation::SchedulerOffloadBridge` is the
//!   real adapter over `FederationHub::dispatch_offload` a caller that owns both a real
//!   `Scheduler` and a real `FederationHub` wires in — see that type's own doc comment. This
//!   crate's one real production caller (`hyperion-agent-runtime::AgentRuntime::prepare_invoke`)
//!   still only ever submits `SchedClass::InteractiveAgent` tasks (never offload-eligible by
//!   design), and `hyperion-memory`'s `run_co_occurrence_pass` (which does submit a real
//!   `BatchDistributable` task) still names no `capability_ref` — no real Model-Router-registered
//!   or federation-reachable Capability exists anywhere in this workspace for that pass yet, so
//!   naming one would be cosmetic, not functional. It also still owns its own private `Scheduler`
//!   instance, separate from `AgentRuntime`'s — assembling one running process that owns both a
//!   `Scheduler` submitting real, capability-named `BatchDistributable` tasks *and* a real
//!   `FederationHub`, and wiring the bridge between them, remains a real, separate composition
//!   decision for whichever binary chooses to do so, not something either library crate should
//!   force on every caller.
//! - ~~**Model-tier degradation.**~~ — now real: `hyperion-model-router::ImplementationDescriptor`
//!   carries a real, optional `ResourceCost` (a narrowed local copy of this crate's own
//!   `ResourceVector` shape, to avoid a dependency cycle — `hyperion-model-router`'s own doc
//!   comment on `ResourceCost` explains why), and [`TaskDescriptor::capability_ref`] names which
//!   capability a task is actually invoking. [`Scheduler::schedule_epoch`]'s non-admit branch now
//!   asks a wired `ModelRouter` (via [`Scheduler::new_with_model_router`]) for every real,
//!   non-`Shadow`, not-circuit-broken registered implementation of that capability that declares
//!   a cost, and admits at the cheapest one that actually fits the real ledgers instead of only
//!   ever aging and requeuing the original request. `hyperion-agent-runtime::AgentRuntime::
//!   prepare_invoke` is the real production caller: it now wires an optional `ModelRouter` in
//!   (`AgentRuntime::new_with_netstack_and_plugins_and_memory_and_model_router`) and names the
//!   invoked capability on every submitted task.
//!
//! - ~~**`Scheduler.subscribeLoadSignal` wiring.**~~ (2026-07-18) — now real:
//!   [`Scheduler::update_load_signal`] is the direct-push counterpart docs/34-observability-
//!   telemetry.md §3 names, and [`Scheduler::current_load_signal`] reads the most recent one
//!   back. `hyperion_observability::scheduler_feedback::publish_load_signal` is the real
//!   production caller — it computes a real [`types::LoadSignal`] (a real EWMA over recorded
//!   CPU-utilization samples, a real derivative over recorded battery-level samples, and the
//!   most recent real thermal-headroom reading) from a real `TelemetryCollector`'s own recorded
//!   `MetricSample`s and pushes it in. No Cargo cycle here (unlike distributed offload above):
//!   `hyperion-observability` doesn't depend on this crate today, and this crate's own full
//!   transitive dependency closure never reaches back to `hyperion-observability`, so a plain,
//!   direct dependency (no trait-object indirection) is enough. Acting on the signal (adaptive
//!   placement/quota decisions) remains a separate, already-named, hardware-blocked deferral —
//!   see [`ledger::ResourceLedger`]'s own doc comment on the thermal/battery feedback governor.
//!
//! Everything else — admission, the ledger, and DRF/EDF dispatch — is exactly what
//! docs/41-implementation-phases.md's Phase 1 exit criteria require.

mod ledger;
mod owner;
mod scheduler;
mod types;

pub use ledger::ResourceLedger;
pub use owner::OwnerAccount;
pub use scheduler::{OffloadTrigger, SchedError, Scheduler, SchedulingRationale};
pub use types::{
    AgentId, IntentId, LoadSignal, OwnerId, ResourceDimension, ResourceVector, SchedClass,
    TaskDescriptor, TaskId, Ticket,
};
