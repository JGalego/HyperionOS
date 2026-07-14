//! Hyperion unified multi-resource scheduler.
//!
//! Implements docs/04-scheduler.md's admission control and Weighted-EDF +
//! Dominant-Resource-Fair dispatch across CPU/RAM/GPU/VRAM/storage/network/
//! inference-tokens/context-window/battery as one multi-dimensional
//! allocation problem, gated by `hyperion-capability` tokens exactly like
//! every other Phase 1 resource.
//!
//! Two of the doc's algorithms are still not implemented, called out at their call site
//! (`schedule_epoch`'s non-admitted branch) rather than silently omitted: model-tier degradation
//! and distributed offload. The doc's own reason for each ("needs Model Router, Phase 3" /
//! "needs Distributed Execution, Phase 7") is now stale — `hyperion-model-router` and
//! `hyperion-federation` both exist — but closing either is a real design question, not a
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
//! - **Model-tier degradation.** `hyperion-model-router::ModelRouter::route` scores registered
//!   implementations by latency/privacy/cost/quality/availability — it has no resource-cost axis
//!   at all, so there's no direct way to ask it "what's a cheaper `ResourceVector` for this
//!   capability." Making this real needs `ImplementationDescriptor` to carry a real resource-cost
//!   dimension and `TaskDescriptor` to carry a capability reference to look one up by — a schema
//!   change to both crates, not a wiring fix to this one.
//!
//! Everything that *is* implemented — admission, the ledger, and DRF/EDF dispatch — is exactly
//! what docs/41-implementation-phases.md's Phase 1 exit criteria require.

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
