use std::collections::HashMap;
use std::time::{Duration, Instant};

use hyperion_capability::{CapabilityMonitor, RightsMask};

use crate::ledger::ResourceLedger;
use crate::owner::OwnerAccount;
use crate::types::{owner_of, OwnerId, ResourceDimension, ResourceVector, SchedClass, TaskDescriptor, TaskId, Ticket};

/// Aging increment applied to a requeued task's `priority_weight` each
/// epoch it fails to be admitted — docs/04-scheduler.md §Recovery
/// Mechanisms: "a task's effective priority_weight increases monotonically
/// with time spent in ready, guaranteeing eventual admission."
const AGING_STEP: f32 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SchedError {
    /// docs/04 §Security Considerations: "the scheduler refuses submit_task
    /// for a request whose token does not authorize" it. This crate's
    /// simplification: any live token with `EXEC` rights authorizes
    /// submission; per-dimension/per-quantity resource rights are a
    /// Phase 8 hardening concern (03-kernel-architecture.md's `RightsMask`
    /// has no resource-quantity vocabulary to check against yet).
    #[error("capability does not authorize task submission")]
    Unauthorized,
    #[error("no such ticket")]
    NoSuchTicket,
}

/// A committed resource grant, tracked so [`Scheduler::complete`] knows
/// exactly what to release.
#[derive(Debug, Clone)]
struct Allocation {
    owner: OwnerId,
    vector: ResourceVector,
}

/// `explain()`'s result — docs/04 §Interfaces / APIs: "a user or Agent can
/// always ask 'why is this slow / why did you use the smaller model' and
/// get the admission-control trace, not silence."
#[derive(Debug, Clone)]
pub struct SchedulingRationale {
    pub ticket: Ticket,
    pub admitted: bool,
    pub note: String,
}

/// The unified scheduler core (docs/04-scheduler.md §Architecture):
/// admission control, per-dimension resource ledgers, and Weighted-EDF +
/// Dominant-Resource-Fair dispatch across every registered
/// [`ResourceDimension`].
#[derive(Default)]
pub struct Scheduler {
    ledgers: HashMap<ResourceDimension, ResourceLedger>,
    owners: HashMap<OwnerId, OwnerAccount>,
    realtime_ready: Vec<TaskDescriptor>,
    /// InteractiveAgent + BackgroundAgent + BatchDistributable all draw
    /// from the same DRF-ranked pool — docs/04 §Algorithms 2 ranks by
    /// cumulative owner share regardless of class, and a `BatchDistributable`
    /// task only reaches the "offer it for offload" branch of
    /// §Pseudocode's `schedule_epoch` by first competing here and losing.
    other_ready: Vec<TaskDescriptor>,
    allocations: HashMap<TaskId, Allocation>,
    rationale: HashMap<TaskId, SchedulingRationale>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// `register_resource_provider` — docs/04 §Interfaces / APIs.
    pub fn register_resource_provider(&mut self, ledger: ResourceLedger) {
        self.ledgers.insert(ledger.dimension, ledger);
    }

    /// `submit_task` — docs/04 §Interfaces / APIs. Queues `task` for the
    /// next [`Self::schedule_epoch`] after checking its capability
    /// authorizes submission at all; admission-control fit is checked at
    /// dispatch time, not here (docs/04 §Algorithms 1: "checked... before
    /// it is queued" refers to the fit check happening pre-dispatch, which
    /// `schedule_epoch` performs on every candidate every epoch).
    pub fn submit_task(&mut self, monitor: &CapabilityMonitor, task: TaskDescriptor) -> Result<Ticket, SchedError> {
        monitor
            .check_rights_ok_result(&task.cap_token, RightsMask::EXEC)
            .map_err(|_| SchedError::Unauthorized)?;

        let ticket = task.id;
        match task.class {
            SchedClass::RealTimeUI => self.realtime_ready.push(task),
            _ => self.other_ready.push(task),
        }
        Ok(ticket)
    }

    /// `cancel` — docs/04 §Interfaces / APIs. Removes a still-pending task,
    /// or releases an already-admitted one's allocation.
    pub fn cancel(&mut self, ticket: Ticket) -> Result<(), SchedError> {
        if let Some(alloc) = self.allocations.remove(&ticket) {
            self.release(&alloc);
            return Ok(());
        }
        if remove_by_id(&mut self.realtime_ready, ticket) || remove_by_id(&mut self.other_ready, ticket) {
            return Ok(());
        }
        Err(SchedError::NoSuchTicket)
    }

    /// `query_ledger` — docs/04 §Interfaces / APIs.
    pub fn query_ledger(&self, dim: ResourceDimension) -> Option<ResourceLedger> {
        self.ledgers.get(&dim).copied()
    }

    /// `explain` — docs/04 §Interfaces / APIs.
    pub fn explain(&self, ticket: Ticket) -> Option<SchedulingRationale> {
        self.rationale.get(&ticket).cloned()
    }

    /// Releases a previously-admitted task's resources back to its ledgers
    /// and its owner's cumulative account — docs/04's `reclaim_completed`.
    /// The real scheduler observes task completion automatically as work
    /// finishes; this simulator has no execution to observe, so callers
    /// signal completion explicitly once their simulated work is done.
    pub fn complete(&mut self, ticket: Ticket) -> Result<(), SchedError> {
        let alloc = self.allocations.remove(&ticket).ok_or(SchedError::NoSuchTicket)?;
        self.release(&alloc);
        Ok(())
    }

    fn release(&mut self, alloc: &Allocation) {
        for (dim, amount) in alloc.vector.iter_dimensions() {
            if let Some(l) = self.ledgers.get_mut(&dim) {
                l.allocated = l.allocated.saturating_sub(amount);
            }
        }
        if let Some(acc) = self.owners.get_mut(&alloc.owner) {
            acc.currently_held.saturating_sub_assign(&alloc.vector);
        }
    }

    /// `schedule_epoch` — docs/04-scheduler.md §Pseudocode. Drains
    /// `RealTimeUI` by earliest deadline against reserved headroom, then
    /// admits the DRF pool in ascending order of cumulative dominant share,
    /// re-sorting after every single admission because it changes that
    /// owner's `currently_held` and therefore every remaining candidate's
    /// share.
    pub fn schedule_epoch(&mut self) -> Vec<SchedulingRationale> {
        let mut report = Vec::new();

        let mut realtime = std::mem::take(&mut self.realtime_ready);
        realtime.sort_by_key(deadline_key);
        for task in realtime {
            match try_admit(&task, &self.ledgers, true) {
                Some(vector) => report.push(self.admit(task, vector)),
                None => report.push(self.miss_realtime_deadline(task)),
            }
        }

        let mut candidates = std::mem::take(&mut self.other_ready);
        while !candidates.is_empty() {
            candidates.sort_by(|a, b| {
                self.dominant_share(a)
                    .partial_cmp(&self.dominant_share(b))
                    .expect("dominant share is always finite")
            });
            let task = candidates.remove(0);
            match try_admit(&task, &self.ledgers, false) {
                Some(vector) => report.push(self.admit(task, vector)),
                None => {
                    // Neither the Model Router (23-multi-model-orchestration.md)
                    // nor Distributed Execution (21-distributed-execution.md)
                    // exist yet, so §Algorithms 4/5's degrade-then-offload
                    // steps aren't implemented — every non-fitting task ages
                    // and is retried next epoch instead (§Recovery Mechanisms).
                    let ticket = task.id;
                    let mut aged = task;
                    aged.priority_weight += AGING_STEP;
                    let rationale = SchedulingRationale {
                        ticket,
                        admitted: false,
                        note: "did not fit this epoch; aged and requeued".to_string(),
                    };
                    self.rationale.insert(ticket, rationale.clone());
                    report.push(rationale);
                    self.other_ready.push(aged);
                }
            }
        }

        report
    }

    fn admit(&mut self, task: TaskDescriptor, vector: ResourceVector) -> SchedulingRationale {
        let ticket = task.id;
        let owner = owner_of(&task);
        for (dim, amount) in vector.iter_dimensions() {
            if let Some(l) = self.ledgers.get_mut(&dim) {
                l.allocated = l.allocated.saturating_add(amount);
            }
        }
        self.owners
            .entry(owner)
            .or_default()
            .currently_held
            .saturating_add_assign(&vector);
        self.allocations.insert(ticket, Allocation { owner, vector });

        let rationale = SchedulingRationale {
            ticket,
            admitted: true,
            note: "admitted".to_string(),
        };
        self.rationale.insert(ticket, rationale.clone());
        rationale
    }

    fn miss_realtime_deadline(&mut self, task: TaskDescriptor) -> SchedulingRationale {
        let rationale = SchedulingRationale {
            ticket: task.id,
            admitted: false,
            note: "missed real-time deadline: reserved headroom insufficient".to_string(),
        };
        self.rationale.insert(task.id, rationale.clone());
        rationale
    }

    /// The DRF ranking value: what this owner's dominant share across all
    /// resource dimensions would become if `task` were admitted *on top of*
    /// every task it already holds — never the task's own request
    /// considered in isolation. docs/04 §Pseudocode's `dominant_share`.
    fn dominant_share(&self, task: &TaskDescriptor) -> f32 {
        let owner = owner_of(task);
        let held = self.owners.get(&owner).map(|a| &a.currently_held);
        task.request
            .iter_dimensions()
            .map(|(d, want)| {
                let already_held = held.map_or(0, |h| h.get(d));
                let capacity = self.ledgers.get(&d).map_or(1, |l| l.capacity).max(1);
                (already_held.saturating_add(want)) as f32 / capacity as f32
            })
            .fold(0.0_f32, f32::max)
            * (1.0 / task.priority_weight.max(0.01))
    }
}

fn try_admit(
    task: &TaskDescriptor,
    ledgers: &HashMap<ResourceDimension, ResourceLedger>,
    use_reserved: bool,
) -> Option<ResourceVector> {
    for (dim, want) in task.request.iter_dimensions() {
        if want == 0 {
            continue;
        }
        let ledger = ledgers.get(&dim)?;
        if ledger.allocated.saturating_add(want) > ledger.headroom(use_reserved) {
            return None;
        }
    }
    Some(task.request)
}

fn remove_by_id(queue: &mut Vec<TaskDescriptor>, ticket: Ticket) -> bool {
    let before = queue.len();
    queue.retain(|t| t.id != ticket);
    queue.len() != before
}

/// Missing deadlines are contractually only possible for non-`RealTimeUI`
/// tasks (docs/04: "required for RealTimeUI, optional hint otherwise"); a
/// `RealTimeUI` task submitted without one is a caller bug, defaulted to
/// "effectively never," so it sorts last rather than panicking.
fn deadline_key(task: &TaskDescriptor) -> Instant {
    task.deadline
        .unwrap_or_else(|| Instant::now() + Duration::from_secs(100 * 365 * 24 * 3600))
}
