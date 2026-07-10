//! Hyperion unified multi-resource scheduler.
//!
//! Implements docs/04-scheduler.md's admission control and Weighted-EDF +
//! Dominant-Resource-Fair dispatch across CPU/RAM/GPU/VRAM/storage/network/
//! inference-tokens/context-window/battery as one multi-dimensional
//! allocation problem, gated by `hyperion-capability` tokens exactly like
//! every other Phase 1 resource.
//!
//! Two of the doc's algorithms are deliberately not implemented yet, and are
//! called out at their call site rather than silently omitted: model-tier
//! degradation (needs docs/23-multi-model-orchestration.md's Model Router,
//! Phase 3) and distributed offload (needs
//! docs/21-distributed-execution.md, Phase 7). Everything that *is*
//! implemented — admission, the ledger, and DRF/EDF dispatch — is exactly
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
