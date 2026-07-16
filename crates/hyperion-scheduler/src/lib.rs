//! Hyperion unified multi-resource scheduler.
//!
//! Implements docs/04-scheduler.md's admission control and Weighted-EDF +
//! Dominant-Resource-Fair dispatch across CPU/RAM/GPU/VRAM/storage/network/
//! inference-tokens/context-window/battery as one multi-dimensional
//! allocation problem, gated by `hyperion-capability` tokens exactly like
//! every other Phase 1 resource.
//!
//! One of the doc's algorithms is still not implemented, called out at its call site
//! (`schedule_epoch`'s non-admitted branch) rather than silently omitted: distributed offload.
//! The doc's own reason ("needs Distributed Execution, Phase 7") is now stale —
//! `hyperion-federation` exists — but closing it is a real design question, not a
//! dependency-missing wiring gap:
//!
//! - **Distributed offload.** `hyperion-federation::FederationHub::dispatch_offload` is real and
//!   already takes a `ResourceVector` to score candidates against — the missing piece isn't
//!   `hyperion-federation`, it's a live trigger: this crate's one real production caller
//!   (`hyperion-agent-runtime::AgentRuntime::prepare_invoke`) only ever submits
//!   `SchedClass::InteractiveAgent` tasks, never `BatchDistributable`, so the "offer it for
//!   offload" branch below has no real task that would ever reach it today. `hyperion-memory`'s
//!   `run_co_occurrence_pass` does submit a real `BatchDistributable` task, but to its own
//!   scheduler instance, not `AgentRuntime`'s, and neither instance is reachable from
//!   `hyperion-federation` without a new architectural decision about which scheduler a
//!   federation-aware offload path should actually watch.
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
//! Everything else — admission, the ledger, and DRF/EDF dispatch — is exactly what
//! docs/41-implementation-phases.md's Phase 1 exit criteria require.

mod ledger;
mod owner;
mod scheduler;
mod types;

pub use ledger::ResourceLedger;
pub use owner::OwnerAccount;
pub use scheduler::{SchedError, Scheduler, SchedulingRationale};
pub use types::{
    AgentId, IntentId, OwnerId, ResourceDimension, ResourceVector, SchedClass, TaskDescriptor,
    TaskId, Ticket,
};
