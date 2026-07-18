use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hyperion_capability::{CapabilityMonitor, RightsMask};
use hyperion_model_router::{ImplId, ModelRouter, ResourceCost};

use crate::ledger::ResourceLedger;
use crate::owner::OwnerAccount;
use crate::types::{
    owner_of, LoadSignal, OwnerId, ResourceDimension, ResourceVector, SchedClass, TaskDescriptor,
    TaskId, Ticket,
};

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

/// This crate's own named "distributed offload" gap (docs/21-distributed-execution.md), made
/// real via dependency injection rather than a direct `hyperion-federation` dependency --
/// `hyperion-federation` already depends on this crate (for `ResourceVector`/`ResourceDimension`),
/// so this crate can never depend back on it without a hard Cargo cycle. A caller that owns both
/// a real `Scheduler` and a real `hyperion_federation::FederationHub` implements this trait as a
/// thin adapter over `FederationHub::dispatch_offload` and wires it in via
/// [`Scheduler::with_offload_trigger`].
pub trait OffloadTrigger: Send + Sync {
    /// Attempts to offload `task` to a real, currently reachable peer device -- only ever called
    /// for a `SchedClass::BatchDistributable` task that already failed both local admission and
    /// model-tier degradation this epoch, and only when `task.capability_ref` names something
    /// real to invoke remotely. Returns the real result on success; `None` on any failure (no
    /// feasible placement, every candidate's own admission refused it, a real network error) --
    /// `schedule_epoch` falls back to aging and requeuing exactly as it already does when no
    /// trigger is wired at all.
    fn try_offload(&self, task: &TaskDescriptor) -> Option<serde_json::Value>;
}

/// `explain()`'s result — docs/04 §Interfaces / APIs: "a user or Agent can
/// always ask 'why is this slow / why did you use the smaller model' and
/// get the admission-control trace, not silence."
#[derive(Debug, Clone)]
pub struct SchedulingRationale {
    pub ticket: Ticket,
    pub admitted: bool,
    pub note: String,
    /// `Some` only when this task completed via a real
    /// [`OffloadTrigger::try_offload`] dispatch to a peer device -- `None` for every
    /// locally-admitted, model-tier-degraded, or aged-and-requeued outcome, which have no result
    /// to report yet (a locally admitted task's actual invocation hasn't happened at this
    /// simulator's scheduling layer; only real offload dispatches synchronously all the way
    /// through to a real result before `schedule_epoch` returns).
    pub offload_result: Option<serde_json::Value>,
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
    /// This crate's own named "model-tier degradation" gap — see [`Self::new_with_model_router`].
    model_router: Option<Arc<ModelRouter>>,
    /// This crate's own named "distributed offload" gap — see [`Self::with_offload_trigger`].
    offload_trigger: Option<Arc<dyn OffloadTrigger>>,
    /// This crate's own named "`Scheduler.subscribeLoadSignal` wiring" gap — see
    /// [`Self::update_load_signal`]/[`Self::current_load_signal`].
    load_signal: Option<LoadSignal>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// As [`Self::new`], additionally wiring a real [`ModelRouter`] so [`Self::schedule_epoch`]'s
    /// non-admit branch can ask it for a cheaper, currently-registered implementation of a task's
    /// own [`TaskDescriptor::capability_ref`] instead of only ever aging and requeuing a request
    /// that doesn't fit — this crate's own doc comment's "model-tier degradation" gap, made real.
    /// `Option`, not a required constructor parameter: most existing callers (this crate's own
    /// tests, `hyperion-memory`'s private scheduler instance) have no Model Router of their own
    /// and shouldn't need to acquire one just to satisfy a parameter they'd never use.
    pub fn new_with_model_router(model_router: Arc<ModelRouter>) -> Self {
        Scheduler {
            model_router: Some(model_router),
            ..Self::default()
        }
    }

    /// Opts this scheduler into a real [`OffloadTrigger`] so [`Self::schedule_epoch`]'s non-admit
    /// branch can offer a `SchedClass::BatchDistributable` task that also failed model-tier
    /// degradation to a real peer device instead of only ever aging and requeuing it -- this
    /// crate's own doc comment's "distributed offload" gap, made real. Chainable after
    /// [`Self::new`]/[`Self::new_with_model_router`], the same builder shape
    /// `hyperion_netstack::NetstackHub::with_ai_runtime` already established: most existing
    /// callers (this crate's own tests, `hyperion-memory`'s private scheduler instance) have no
    /// federation hub to wire and should not be forced to acquire one.
    pub fn with_offload_trigger(mut self, trigger: Arc<dyn OffloadTrigger>) -> Self {
        self.offload_trigger = Some(trigger);
        self
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
    pub fn submit_task(
        &mut self,
        monitor: &CapabilityMonitor,
        task: TaskDescriptor,
    ) -> Result<Ticket, SchedError> {
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
        if remove_by_id(&mut self.realtime_ready, ticket)
            || remove_by_id(&mut self.other_ready, ticket)
        {
            return Ok(());
        }
        Err(SchedError::NoSuchTicket)
    }

    /// `query_ledger` — docs/04 §Interfaces / APIs.
    pub fn query_ledger(&self, dim: ResourceDimension) -> Option<ResourceLedger> {
        self.ledgers.get(&dim).copied()
    }

    /// docs/34-observability-telemetry.md §3's own `Scheduler.subscribeLoadSignal` -- this
    /// crate's own previously-named "no subscription API to receive one" gap, closed as a plain,
    /// direct push rather than an invented callback/subscriber-list mechanism: this crate has no
    /// event loop of its own to invoke callbacks from (every method here runs synchronously,
    /// called by whichever caller owns this `Scheduler`), and exactly one real publisher exists
    /// in this workspace (`hyperion_observability::scheduler_feedback`) -- a subscriber registry
    /// built for a second publisher that doesn't exist yet would be exactly the "looks real,
    /// never actually exercised" surface this workspace's own testing discipline rules out. A
    /// caller that owns both a real `Scheduler` and a real `hyperion_observability::
    /// TelemetryCollector` calls this each time a new signal is computed, the same "callers signal
    /// explicitly, this simulator has no execution to observe automatically" shape
    /// [`Self::complete`] already established.
    pub fn update_load_signal(&mut self, signal: LoadSignal) {
        self.load_signal = Some(signal);
    }

    /// The most recent [`LoadSignal`] pushed via [`Self::update_load_signal`], if any -- `None`
    /// before any real caller has ever pushed one. Acting on it (adaptive placement/quota
    /// decisions) is a separate, already-named, hardware-blocked deferral (see
    /// [`crate::ledger::ResourceLedger`]'s own doc comment on the thermal/battery feedback
    /// governor) this method does not itself attempt.
    pub fn current_load_signal(&self) -> Option<LoadSignal> {
        self.load_signal
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
        let alloc = self
            .allocations
            .remove(&ticket)
            .ok_or(SchedError::NoSuchTicket)?;
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
                Some(vector) => report.push(self.admit(task, vector, "admitted".to_string())),
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
                Some(vector) => report.push(self.admit(task, vector, "admitted".to_string())),
                None => {
                    // Model-tier degradation (23-multi-model-orchestration.md) is real: a wired
                    // ModelRouter is asked for a cheaper registered implementation of this task's
                    // own capability first (preferring a cheaper *local* substitute over a
                    // network round trip, per this workspace's own "Local First" principle).
                    // Distributed offload (21-distributed-execution.md) is also real now: a wired
                    // OffloadTrigger is asked to run a BatchDistributable task on a real peer
                    // device before finally falling back to aging and requeuing.
                    match self.try_degrade(&task) {
                        Some((impl_id, vector)) => {
                            let note = format!(
                                "did not fit at its own declared request; admitted at cheaper \
                                 registered implementation {} instead",
                                impl_id.0
                            );
                            report.push(self.admit(task, vector, note));
                        }
                        None => match self.try_offload(&task) {
                            Some(result) => {
                                let ticket = task.id;
                                let rationale = SchedulingRationale {
                                    ticket,
                                    admitted: true,
                                    note: "did not fit locally; completed via real federation \
                                           offload to a peer device"
                                        .to_string(),
                                    offload_result: Some(result),
                                };
                                self.rationale.insert(ticket, rationale.clone());
                                report.push(rationale);
                                // No local Allocation is created -- the real work already
                                // completed on the peer device's own resources, never this
                                // device's ledgers.
                            }
                            None => {
                                let ticket = task.id;
                                let mut aged = task;
                                aged.priority_weight += AGING_STEP;
                                let rationale = SchedulingRationale {
                                    ticket,
                                    admitted: false,
                                    note: "did not fit this epoch; aged and requeued".to_string(),
                                    offload_result: None,
                                };
                                self.rationale.insert(ticket, rationale.clone());
                                report.push(rationale);
                                self.other_ready.push(aged);
                            }
                        },
                    }
                }
            }
        }

        report
    }

    /// This crate's own named "model-tier degradation" gap, made real: when `task` didn't fit at
    /// its own declared [`TaskDescriptor::request`], ask the wired [`ModelRouter`] (if any) for
    /// every real, currently-registered, non-circuit-broken implementation of `task`'s own
    /// [`TaskDescriptor::capability_ref`] (if any) that declares a resource cost, and return the
    /// cheapest one (by total declared footprint) that actually fits the real ledgers — never an
    /// invented substitute. `None` if no router is wired, the task names no capability, or
    /// nothing registered for it fits either.
    fn try_degrade(&self, task: &TaskDescriptor) -> Option<(ImplId, ResourceVector)> {
        let model_router = self.model_router.as_ref()?;
        let capability_ref = task.capability_ref.as_deref()?;
        let mut candidates: Vec<(ImplId, ResourceVector)> = model_router
            .declared_costs(capability_ref)
            .into_iter()
            .map(|(impl_id, cost)| (impl_id, from_declared_cost(cost)))
            .collect();
        candidates.sort_by_key(|(_, vector)| total_footprint(vector));
        candidates
            .into_iter()
            .find(|(_, vector)| fits(vector, &self.ledgers, false))
    }

    /// This crate's own named "distributed offload" gap, made real: only ever attempted for a
    /// [`SchedClass::BatchDistributable`] task (docs/04's own class distinction -- an
    /// `InteractiveAgent`/`BackgroundAgent` task is never a candidate) that names a real
    /// [`TaskDescriptor::capability_ref`] to invoke remotely. `None` if no trigger is wired, the
    /// task isn't `BatchDistributable`, it names no capability, or the trigger itself couldn't
    /// place it anywhere.
    fn try_offload(&self, task: &TaskDescriptor) -> Option<serde_json::Value> {
        if task.class != SchedClass::BatchDistributable {
            return None;
        }
        task.capability_ref.as_ref()?;
        self.offload_trigger.as_ref()?.try_offload(task)
    }

    fn admit(
        &mut self,
        task: TaskDescriptor,
        vector: ResourceVector,
        note: String,
    ) -> SchedulingRationale {
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
        self.allocations
            .insert(ticket, Allocation { owner, vector });

        let rationale = SchedulingRationale {
            ticket,
            admitted: true,
            note,
            offload_result: None,
        };
        self.rationale.insert(ticket, rationale.clone());
        rationale
    }

    fn miss_realtime_deadline(&mut self, task: TaskDescriptor) -> SchedulingRationale {
        let rationale = SchedulingRationale {
            ticket: task.id,
            admitted: false,
            note: "missed real-time deadline: reserved headroom insufficient".to_string(),
            offload_result: None,
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

/// Whether `vector` fits against `ledgers`' remaining headroom — every dimension `vector`
/// requests a nonzero amount of must both have a registered ledger and have room, exactly
/// [`try_admit`]'s own per-task check, factored out so [`Scheduler::try_degrade`] can run the
/// identical real admission check against a substitute vector instead of a task's own request.
fn fits(
    vector: &ResourceVector,
    ledgers: &HashMap<ResourceDimension, ResourceLedger>,
    use_reserved: bool,
) -> bool {
    for (dim, want) in vector.iter_dimensions() {
        if want == 0 {
            continue;
        }
        let Some(ledger) = ledgers.get(&dim) else {
            return false;
        };
        if ledger.allocated.saturating_add(want) > ledger.headroom(use_reserved) {
            return false;
        }
    }
    true
}

fn try_admit(
    task: &TaskDescriptor,
    ledgers: &HashMap<ResourceDimension, ResourceLedger>,
    use_reserved: bool,
) -> Option<ResourceVector> {
    fits(&task.request, ledgers, use_reserved).then_some(task.request)
}

/// Field-for-field conversion from `hyperion-model-router`'s own narrowed
/// [`hyperion_model_router::ResourceCost`] to this crate's [`ResourceVector`] — see
/// `hyperion_model_router::types::ResourceCost`'s own doc comment for why this lives here
/// (this crate depends on that one, never the reverse) rather than as a shared type.
fn from_declared_cost(cost: ResourceCost) -> ResourceVector {
    ResourceVector {
        cpu_shares: cost.cpu_shares,
        ram_mb: cost.ram_mb,
        gpu_shares: cost.gpu_shares,
        vram_mb: cost.vram_mb,
        storage_iops: cost.storage_iops,
        network_bw_kbps: cost.network_bw_kbps,
        inference_tokens_per_sec: cost.inference_tokens_per_sec,
        context_window_slots: cost.context_window_slots,
        battery_budget_mw: cost.battery_budget_mw,
    }
}

/// The ranking [`Scheduler::try_degrade`] picks the *cheapest* fitting alternative by — summed
/// across every dimension, since which single dimension is scarce varies per ledger
/// configuration and this is only used to order candidates, never to check admission itself
/// ([`fits`] does that, per-dimension, against the real ledgers).
fn total_footprint(vector: &ResourceVector) -> u64 {
    vector.iter_dimensions().map(|(_, v)| u64::from(v)).sum()
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
