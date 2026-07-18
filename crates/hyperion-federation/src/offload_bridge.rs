//! `hyperion-scheduler`'s own named "distributed offload" gap, closed from this crate's side: a
//! real [`hyperion_scheduler::OffloadTrigger`] implementation over [`crate::FederationHub::
//! dispatch_offload`], the real dispatch mechanism that already existed with nothing left to
//! trigger it. `hyperion-scheduler` can never depend on this crate directly (this crate already
//! depends on `hyperion-scheduler`, for `ResourceVector`/`ResourceDimension` — a reverse
//! dependency would be a hard Cargo cycle), so this bridge is the real adapter a caller that owns
//! both a real `Scheduler` and a real `FederationHub` wires in via `Scheduler::
//! with_offload_trigger`.

use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use hyperion_capability::CapabilityMonitor;
use hyperion_scheduler::{OffloadTrigger, TaskDescriptor};

use crate::hub::FederationHub;
use crate::types::{OffloadDescriptor, PrivacyTier};

fn real_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after the Unix epoch")
        .as_secs()
}

/// The real bridge between a `Scheduler`'s own `SchedClass::BatchDistributable` tasks and this
/// crate's real, peer-reachable `FederationHub::dispatch_offload`. Holds its own
/// `CapabilityMonitor` reference rather than requiring `Scheduler::schedule_epoch` to grow one
/// (that method takes none today, and every other existing caller would have to acquire one it
/// never needed) -- the caller assembling both a real `Scheduler` and a real `FederationHub`
/// already has the one real monitor both need to share.
pub struct SchedulerOffloadBridge {
    hub: Arc<FederationHub>,
    monitor: Arc<CapabilityMonitor>,
}

impl SchedulerOffloadBridge {
    pub fn new(hub: Arc<FederationHub>, monitor: Arc<CapabilityMonitor>) -> Self {
        SchedulerOffloadBridge { hub, monitor }
    }
}

impl OffloadTrigger for SchedulerOffloadBridge {
    /// Builds a real [`OffloadDescriptor`] from `task`'s own real fields (`request`, `deadline`,
    /// `cap_token`) and dispatches it via [`FederationHub::dispatch_offload`] -- `task`'s own
    /// [`TaskDescriptor::cap_token`] authorizes the call, exactly the token the scheduler itself
    /// already checked for `RightsMask::EXEC` at submission time. `privacy_tier` defaults to the
    /// most restrictive [`PrivacyTier::Local`] -- `TaskDescriptor` carries no privacy signal of
    /// its own, and "assume the safe option when unspecified" matches this workspace's own
    /// deny-by-default convention elsewhere (`hyperion-privacy`'s "never assume consent"). Any
    /// real failure (no feasible placement, every candidate's own admission refused it, a real
    /// network error) is logged and returned as `None`, never silently fabricated as success.
    fn try_offload(&self, task: &TaskDescriptor) -> Option<serde_json::Value> {
        let capability_ref = task.capability_ref.as_deref()?;
        let deadline_ms = task.deadline.map(|deadline| {
            deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(u128::from(u32::MAX)) as u32
        });
        let descriptor = OffloadDescriptor {
            request: task.request,
            deadline_ms,
            privacy_tier: PrivacyTier::Local,
        };
        match self.hub.dispatch_offload(
            &self.monitor,
            &task.cap_token,
            &descriptor,
            capability_ref,
            task.args.clone(),
            task.owner_intent.0,
            real_now(),
        ) {
            Ok(result) => Some(result),
            Err(e) => {
                eprintln!(
                    "hyperion-federation: real offload dispatch for task {:?} (capability {capability_ref:?}) \
                     failed, falling back to local aging/requeue: {e}",
                    task.id
                );
                None
            }
        }
    }
}
