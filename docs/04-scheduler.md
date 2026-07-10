# Scheduler

## Purpose

This document specifies Hyperion's unified resource scheduler: the single subsystem that decides,
moment to moment, which classical resources (CPU, RAM, storage, battery, network, thermal budget)
and which AI-specific resources (VRAM, inference token throughput, context-window occupancy, model
tier routing, agent priority) are granted to which unit of work. It implements the GPU and
inference scheduling classes introduced as kernel-level concerns in
[03 — Kernel Architecture](03-kernel-architecture.md), and it is the consumer of capability tokens
minted there — no resource is ever granted to a caller who cannot present the token for it. It
takes priority and deadline input from [05 — Intent Engine](05-intent-engine.md), resource requests
from [11 — Agent Runtime](11-agent-runtime.md), model-tier requirements from
[23 — Multi-Model Orchestration](23-multi-model-orchestration.md), and offload decisions to
[21 — Distributed Execution](21-distributed-execution.md).

## Motivation

[01 — Vision & Philosophy §10](01-vision-and-philosophy.md#10-success-criteria) requires Hyperion
to feel instantaneous while running continuous background reasoning, on hardware ranging from a
Raspberry Pi to an enterprise cluster. This is impossible if CPU/RAM/network are scheduled by one
subsystem and GPU/inference/model-routing are scheduled by another, bolted-on, best-effort layer —
the two subsystems would silently starve each other. A voice query and a background Research
[Agent](02-core-architecture.md#agent) do not compete for different atoms of silicon; they compete
for the *same* CPU cores, the *same* memory bus, and often the *same* inference accelerator. Only a
scheduler that reasons about classical and AI resources as **one multi-dimensional allocation
problem** can guarantee that an always-on reasoning substrate never degrades interactive
responsiveness — which is the [Golden Rule](01-vision-and-philosophy.md#2-the-golden-rule) applied
to resource management: a scheduling decision that makes a human wait on a spinner because a
background Agent claimed the GPU is a design failure, not an acceptable trade-off.

## Architecture

```
                     ┌─────────────────────────────────────────────┐
                     │   05 Intent Engine  →  priority, deadline    │
                     │   11 Agent Runtime   →  resource requests    │
                     │   23 Model Router    →  tier ↔ ResourceVector│
                     │   21 Distributed Exec→  offload candidates   │
                     └───────────────────┬───────────────────────────┘
                                         ▼
┌───────────────────────────────────────────────────────────────────────────┐
│                          UNIFIED SCHEDULER CORE (L0/L1)                    │
│                                                                             │
│  ┌────────────────┐   ┌───────────────────┐   ┌─────────────────────────┐ │
│  │ Admission       │──▶│ Resource Ledger    │──▶│ Thermal/Battery         │ │
│  │ Controller      │   │ (per-dimension     │   │ Feedback Governor       │ │
│  │ (fit + degrade) │   │  capacity/alloc)   │   │ (scales ledger capacity)│ │
│  └────────────────┘   └───────────────────┘   └─────────────────────────┘ │
│           │                                                                │
│           ▼                                                                │
│  ┌─────────────────────────────────────────────────────────────────────┐  │
│  │  Dispatch: Weighted-EDF (Real-Time UI) + Dominant-Resource Fair      │  │
│  │  Share (Interactive/Background Agent) + Best-Effort (Batch)          │  │
│  └─────────────────────────────────────────────────────────────────────┘  │
└───────────┬───────────────┬───────────────┬───────────────┬───────────────┘
            ▼               ▼               ▼               ▼
     ┌───────────┐   ┌────────────┐  ┌──────────────┐  ┌────────────────────┐
     │ CPU / RAM  │   │ GPU / VRAM │  │ Inference     │  │ Storage / Network /│
     │ dispatch   │   │ dispatch   │  │ token budget  │  │ Distributed offload│
     │ (03-kernel)│   │ (03-kernel)│  │ + context-win │  │ (21-distributed-   │
     └───────────┘   └────────────┘  └──────────────┘  │  execution.md)      │
                                                          └────────────────────┘
```

The scheduler sits at L0/L1 in [02's layer view](02-core-architecture.md#1-layered-system-view):
its dispatch mechanisms (which physical core or accelerator queue runs next) are kernel-provided
per [03 — Kernel Architecture](03-kernel-architecture.md); its policy — *which task deserves that
mechanism next, across every resource dimension at once* — is the subject of this document and is
the "System Runtime" entry in L1.

## Data Structures

```rust
/// One resource request or allocation, spanning classical AND AI-specific
/// dimensions. Every task is described in this vector, never in CPU-only terms.
struct ResourceVector {
    cpu_shares: u32,             // weighted CPU time, DRF-normalized
    ram_mb: u32,
    gpu_shares: u32,             // weighted GPU compute time
    vram_mb: u32,
    storage_iops: u32,
    network_bw_kbps: u32,
    inference_tokens_per_sec: u32,
    context_window_slots: u32,   // occupancy in the active Context Bundle's window
    battery_budget_mw: u32,
}

enum SchedClass {
    RealTimeUI,          // EDF; sub-frame deadlines, e.g. compositor/input latency
    InteractiveAgent,     // DRF-weighted; user is waiting on the result
    BackgroundAgent,      // DRF-weighted, lower base weight; user is not waiting
    BatchDistributable,   // best-effort; first candidate for 21-distributed-execution.md offload
}

struct TaskDescriptor {
    id: TaskId,
    owner_intent: IntentId,          // from 05-intent-engine.md
    owner_agent: Option<AgentId>,    // from 11-agent-runtime.md
    class: SchedClass,
    deadline: Option<Instant>,       // required for RealTimeUI, optional hint otherwise
    priority_weight: f32,            // derived from Intent priority + Agent priority
    request: ResourceVector,
    model_tier_hint: Option<ModelTier>,  // scheduler-local sizing hint, see Data Structures below
    cap_token: CapabilityToken,       // per 03-kernel-architecture.md; unforgeable, revocable
}

/// Per-resource-dimension ledger; one instance per physical/thermal domain.
struct ResourceLedger {
    dimension: ResourceDimension,
    capacity: u32,                    // current allocatable capacity (post-thermal-scaling)
    reserved_for_realtime: u32,       // headroom carved out, never given to batch/background
    allocated: u32,
    epoch: u64,
}

/// A ResourceProfile is the *declared, static* resource shape a manifest (Agent, Plugin, or
/// SDK Implementation) advertises before it has ever run — e.g. "this Capability typically
/// needs ~200MB RAM and a small local model." A ResourceVector is the *concrete, runtime*
/// instance of that shape for one specific task admission. `resolve()` is what compiles a
/// declared profile down to a live vector at submit_task time, folding in the actual model
/// tier the Model Router (23) picked for this call. 03-kernel-architecture.md's
/// `sandbox_create`, 11-agent-runtime.md's `AgentManifest`, 24-plugin-framework.md's
/// `PluginManifest`, and 25-sdk.md's `Implementation` all declare a `ResourceProfile`; only
/// this scheduler ever turns one into a `ResourceVector`.
struct ResourceProfile {
    dimension_estimates: HashMap<ResourceDimension, Range<u32>>,  // typical..worst-case
    default_class: SchedClass,
    model_tier: Option<ModelTier>,    // see below; None for non-AI Capabilities
}
impl ResourceProfile {
    fn resolve(&self, chosen_tier: Option<ModelTier>) -> ResourceVector { /* worst-case estimate,
        narrowed by chosen_tier's concrete footprint per 23-multi-model-orchestration.md */ }
}

/// Coarse sizing tier a Capability contract or manifest can hint at before the Model Router
/// (23) has resolved a specific `ImplementationDescriptor`. This is deliberately coarser than
/// 23's `ImplKind` (LocalSmallModel/LocalLargeModel/CloudAPI/NativeBinary/Composed): a
/// `ModelTier` is a *resource-sizing* hint the scheduler uses for early admission estimates,
/// before 23 has necessarily chosen a specific implementation; 23's routing decision is what
/// ultimately determines the concrete `ResourceVector`, and `ImplKind` plus 22's `ModelClass`
/// (the model's functional family — SLM/LRM/Vision/Speech/Coding/Planning) together determine
/// which vector `ResourceProfile::resolve` returns.
enum ModelTier { Light, Standard, Heavy }

/// Cumulative resource consumption *currently held* by one scheduling owner (an Agent, or a
/// bare Intent if no Agent owns the task) — not a historical sum. This is the quantity real
/// Dominant Resource Fairness ranks by: a user/owner's dominant share is their current
/// allocation in their dominant dimension divided by that dimension's capacity, and the next
/// admission slot goes to whichever owner's *current* dominant share is lowest. Tracking this
/// per-owner (not per-task) is what makes DRF strategy-proof: an owner cannot gain share by
/// splitting one large request into many small ones, because `currently_held` sums across
/// every in-flight task that owner has, however it's split.
/// The unit DRF fairness is computed per — an Agent if one owns the task, otherwise the bare
/// Intent that submitted it directly (e.g. a Capability invoked without an Agent in the loop).
enum OwnerId {
    Agent(AgentId),      // 11-agent-runtime.md
    Intent(IntentId),    // 05-intent-engine.md
}

struct OwnerAccount {
    owner: OwnerId,
    currently_held: ResourceVector,   // sum of ResourceVectors of this owner's in-flight tasks
}
```

## Algorithms

**1. Admission control.** Every `submit_task` call is checked against every `ResourceLedger`
dimension it touches *before* it is queued, not after it starts running. A task fits if, for every
dimension `d` in its `ResourceVector`, `allocated[d] + request[d] <= capacity[d] - reserved_for_realtime[d]`
(the real-time reservation is only bypassed by `SchedClass::RealTimeUI` tasks themselves). A task
that does not fit is **degraded, not rejected**, per
[02 §4's](02-core-architecture.md#4-design-invariants) "degrade, never fail closed" invariant: the
admission controller asks [23 — Multi-Model Orchestration](23-multi-model-orchestration.md) for a
cheaper tier of the same Capability (smaller local model instead of a large one, lower
`inference_tokens_per_sec` and `vram_mb`), retries the fit check, and only falls back to queuing
or to [21 — Distributed Execution](21-distributed-execution.md) offload if no local degradation
fits within the task's deadline.

**2. Dispatch: Weighted-EDF + Dominant Resource Fairness hybrid.** `RealTimeUI` tasks are scheduled
by Earliest-Deadline-First against the reserved headroom — this is what guarantees compositor and
input-handling latency regardless of background AI load. All other classes share the remaining
capacity by a generalization of Dominant Resource Fairness (DRF) across the *full* `ResourceVector`,
not just CPU. Classical DRF's fairness and strategy-proofness guarantees come from ranking by an
*owner's currently-held cumulative allocation*, not any single task's request size — a scheduler
that ranked per-task requests instead would let an owner win repeatedly just by submitting many
small tasks, since each one would individually look "cheap" relative to capacity. Hyperion's
scheduler therefore tracks one `OwnerAccount.currently_held` vector per Agent (or bare Intent, for
tasks with no owning Agent) and defines a task's **dominant share** as `max over d of
((owner.currently_held[d] + request[d]) / capacity[d])` — what the owner's dominant share *would
become* if this task were admitted on top of everything else it currently holds — weighted by
`priority_weight`, itself a function of Intent priority ([05](05-intent-engine.md)) and Agent
priority ([11](11-agent-runtime.md)). The scheduler admits in ascending order of this value and
increments `currently_held` on admission, decrementing it again when the task completes or its
allocation is released (§Pseudocode). This is the generalization DRF's authors designed it for: it
extends cleanly from CPU+RAM to CPU+RAM+VRAM+tokens+context-window-slots without a new fairness
definition per resource, and it preserves the anti-gaming property that makes DRF worth using in
the first place.

**3. Thermal/battery feedback governor.** Every governor tick (default 100 ms), the kernel's
sensor readings scale each `ResourceLedger.capacity` down from its nameplate maximum by a
throttle factor computed by a bounded control loop (a PI controller against a thermal setpoint,
not an on/off cutoff, to avoid oscillation — see [Failure Modes](#failure-modes)). This is what
lets sustained inference load throttle *before* it hits a thermal cutoff that would also stall the
UI, and what makes battery-aware degradation ("running on battery, prefer the local small model")
a first-class scheduling input rather than an application-level heuristic.

**4. Model-tier coupling.** When a task's `model_tier_hint` names a Capability rather than a fixed
`ResourceVector`, the scheduler asks [23 — Multi-Model Orchestration](23-multi-model-orchestration.md)
to resolve it into a concrete vector for one or more candidate tiers (edge/local vs. larger
local vs. consented cloud), in priority order consistent with
[02 §4's](02-core-architecture.md#4-design-invariants) local-first invariant; admission control
retries fit against each candidate in order.

**5. Offload decision.** If no local tier fits within deadline even after degradation, the task —
if its class is `BatchDistributable` or if the Agent explicitly allows offload — is handed to
[21 — Distributed Execution](21-distributed-execution.md), which treats a remote device as a
virtual `ResourceLedger` with an added network-latency term folded into the EDF deadline check, so
"offload" is not a separate scheduling regime but one more admission candidate.

## Interfaces / APIs

```
submit_task(desc: TaskDescriptor) -> Ticket
cancel(ticket: Ticket) -> Result<(), SchedError>
query_ledger(dim: ResourceDimension) -> ResourceLedgerSnapshot
set_intent_deadline(intent: IntentId, deadline: Instant)          // from 05-intent-engine.md
hint_agent_priority(agent: AgentId, weight: f32)                  // from 11-agent-runtime.md
register_resource_provider(domain: ThermalDomain, ledger: ResourceLedger)  // from 03-kernel-architecture.md
propose_offload_candidate(ticket: Ticket) -> OffloadDescriptor     // to 21-distributed-execution.md
explain(ticket: Ticket) -> SchedulingRationale                    // for 18-explainability-and-trust.md
```

`explain()` exists because [01 §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable)
requires every autonomous action to be explainable on demand — a user or Agent can always ask "why
is this slow / why did you use the smaller model" and get the admission-control trace, not silence.

## Pseudocode

```rust
fn schedule_epoch(ledgers: &mut HashMap<ResourceDimension, ResourceLedger>,
                   owners: &mut HashMap<OwnerId, OwnerAccount>,
                   ready: &mut PriorityQueues) {
    governor_tick(ledgers);                       // thermal/battery scaling, §Algorithms 3
    reclaim_completed(owners, ledgers);            // decrement currently_held for tasks that
                                                    // finished or released since the last epoch

    // Real-time class always drains first, against reserved headroom only.
    while let Some(task) = ready.realtime.pop_earliest_deadline() {
        if let Some(alloc) = try_admit(&task, ledgers, /*use_reserved=*/true) {
            dispatch(task, alloc, owners);
        } else {
            // Missing an RT-UI deadline is itself a failure mode (see below):
            // escalate rather than silently drop a frame.
            escalate_missed_deadline(task);
        }
    }

    // Interactive and background classes share remaining capacity via DRF, re-sorted after
    // every admission (not once up front) because each admission changes its owner's
    // currently_held and therefore every remaining task's dominant share.
    let mut candidates: Vec<_> = ready.interactive.drain().chain(ready.background.drain()).collect();
    while !candidates.is_empty() {
        candidates.sort_by(|a, b|
            dominant_share(a, ledgers, owners).partial_cmp(&dominant_share(b, ledgers, owners)).unwrap());
        let task = candidates.remove(0);
        match try_admit(&task, ledgers, /*use_reserved=*/false) {
            Some(alloc) => dispatch(task, alloc, owners),
            None => {
                if let Some(degraded) = try_degrade_via_model_router(&task) {
                    ready.push_front(degraded);   // retry next epoch at a cheaper tier
                } else if task.class == SchedClass::BatchDistributable {
                    let offer = propose_offload_candidate(task.ticket());
                    distributed_execution::submit(offer);   // 21-distributed-execution.md
                } else {
                    ready.requeue_with_aging(task);          // anti-starvation, see Failure Modes
                }
            }
        }
    }
}

fn try_admit(task: &TaskDescriptor, ledgers: &HashMap<ResourceDimension, ResourceLedger>,
             use_reserved: bool) -> Option<Allocation> {
    for (dim, want) in task.request.iter_dimensions() {
        let l = ledgers.get(&dim)?;
        let headroom = if use_reserved { l.capacity } else { l.capacity - l.reserved_for_realtime };
        if l.allocated + want > headroom { return None; }
    }
    Some(Allocation::commit(task, ledgers))   // atomically reserves across every dimension
}

fn owner_of(task: &TaskDescriptor) -> OwnerId {
    task.owner_agent.map(OwnerId::Agent).unwrap_or(OwnerId::Intent(task.owner_intent))
}

// The DRF ranking value: what this owner's dominant share across all resource dimensions
// would become if `task` were admitted *on top of* every task this owner already holds —
// never the task's own request considered in isolation. This is what makes the ranking a
// genuine cumulative-allocation fairness measure rather than a per-task heuristic.
fn dominant_share(task: &TaskDescriptor, ledgers: &HashMap<ResourceDimension, ResourceLedger>,
                   owners: &HashMap<OwnerId, OwnerAccount>) -> f32 {
    let held = owners.get(&owner_of(task)).map(|a| &a.currently_held);
    task.request.iter_dimensions()
        .map(|(d, want)| {
            let already_held = held.map_or(0, |h| h.get(d));
            (already_held + want) as f32 / ledgers[&d].capacity as f32
        })
        .fold(0.0, f32::max) * (1.0 / task.priority_weight.max(0.01))
}

fn dispatch(task: TaskDescriptor, alloc: Allocation, owners: &mut HashMap<OwnerId, OwnerAccount>) {
    owners.entry(owner_of(&task)).or_default().currently_held += &alloc.vector;
    execute(task, alloc);   // hands off to the Agent Runtime (11) / Capability invocation
}

// Called once per epoch, before admission: any task that finished or was cancelled since
// the last epoch releases its share back to its owner's account, so a completed task's
// resource footprint stops counting against that owner's future dominant-share ranking.
fn reclaim_completed(owners: &mut HashMap<OwnerId, OwnerAccount>,
                      ledgers: &mut HashMap<ResourceDimension, ResourceLedger>) {
    for (owner, released) in drain_completed_allocations() {
        owners.entry(owner).or_default().currently_held -= &released;
    }
}
```

## Security Considerations

Every `TaskDescriptor` carries the `cap_token` minted by
[03 — Kernel Architecture](03-kernel-architecture.md) for its owning Trust Boundary; the scheduler
refuses `submit_task` for a request whose token does not authorize the resource dimensions or
quantities requested — an Agent cannot request more VRAM or a larger model tier than its granted
resource profile allows, closing off a whole class of privilege-escalation-via-resource-exhaustion.
Per-Agent and per-Capability quotas prevent a single malicious or buggy Agent from starving the
system (a runaway Research Agent cannot consume the entire inference token budget, because its
`ResourceLedger` allocation is capped independent of how many tasks it submits). Because scheduling
timing itself can leak information across a Trust Boundary (did a competing security-sensitive task
run, and for how long), the DRF dispatcher performs core-scheduling isolation consistent with
[03 — Kernel Architecture §Security](03-kernel-architecture.md#security-considerations): tasks from
different security domains are never given inferable relative timing on shared execution units.
Thermal and battery telemetry inputs to the governor are themselves capability-scoped reads, so a
Capability cannot spoof sensor data to force favorable throttling. Full threat coverage is in
[17 — Threat Model](17-threat-model.md); policy composition is in
[15 — Security Architecture](15-security-architecture.md).

## Failure Modes

- **Starvation.** Low-`priority_weight` background Agents could starve indefinitely under DRF if
  interactive load is sustained; addressed by aging (below).
- **Priority inversion.** An `InteractiveAgent` task blocked on an IPC reply from a
  `BackgroundAgent`-class server (see [30 — IPC Framework](30-ipc-framework.md)) can inherit its
  waiter's effective priority to avoid inversion.
- **Thermal oscillation.** An on/off throttle would cause the governor to hunt between full and
  throttled capacity; mitigated by the PI-controlled, hysteresis-bounded throttle factor in
  §Algorithms 3.
- **Missed real-time deadlines.** If the reserved headroom itself is insufficient (e.g., a
  misconfigured device profile), RT-UI tasks miss deadlines even before touching shared capacity —
  this is escalated as a system health event, never silently dropped.
- **Distributed offload partition.** A network partition mid-offload leaves a task neither
  running locally (evicted to make room) nor completing remotely.
- **Model-tier mismatch surprise.** Silent degradation to a smaller model could produce
  unexpectedly low-quality output without the user understanding why.

## Recovery Mechanisms

Starvation is bounded by an **aging term**: a task's effective `priority_weight` increases
monotonically with time spent in `ready`, guaranteeing eventual admission (classic aging, tuned so
that no task waits longer than a bounded multiple of its class's target latency). Missed RT-UI
deadlines trigger a watchdog that raises the reserved-headroom floor for one subsequent epoch and
logs the event to [34 — Observability & Telemetry](34-observability-telemetry.md). Offloaded tasks
that lose network connectivity are checkpointed and either resumed locally (if a partial result
exists) or restarted using the recovery-point mechanism in
[33 — Rollback & Recovery](33-rollback-recovery.md) — never left in an undefined state. Every
degradation decision the admission controller makes is logged and retrievable via `explain()`, so a
model-tier mismatch is always answerable through
[18 — Explainability & Trust](18-explainability-and-trust.md) rather than left as unexplained
quality loss, consistent with [02 §4's](02-core-architecture.md#4-design-invariants) explainability
invariant.

## Performance Analysis

Admission control is `O(R)` per task, where `R` is the small, fixed number of resource dimensions
in `ResourceVector` (nine, above) — independent of the number of other tasks in the system, which
is what keeps admission cheap enough to run on every `submit_task` call rather than only
periodically. DRF re-ranking is `O(n log n)` in the number of ready tasks per epoch; at the target
epoch length (5–10 ms for the RT-UI-adjacent tier, ~100 ms for background rebalancing, matching the
governor tick), this comfortably fits within the sub-second workspace-generation and near-instant
wake targets set in [36 — Performance Benchmarks](36-performance-benchmarks.md). The reserved
real-time headroom is sized so that compositor/input latency is provably independent of background
Agent load — the central performance claim this design makes over a CPU-only scheduler with AI
work layered opportunistically on top, which cannot make that guarantee because it has no concept
of a shared, jointly-reserved multi-resource budget.

## Trade-offs

A single unified ledger across nine heterogeneous resource dimensions is more complex to reason
about and to keep from becoming a contention hot spot than separate CPU and GPU schedulers running
independently — Hyperion accepts this because separate schedulers cannot express the joint
constraint ("this Agent's inference call and that process's CPU burst must not together blow the
thermal budget") that is the entire point of §Motivation. At scale, the ledger is sharded per
NUMA/thermal domain (mirroring the `register_resource_provider` per-domain design) rather than kept
as one global lock, trading a small amount of cross-domain fairness precision for scalability up to
the enterprise-cluster end of [37 — Scalability Roadmap](37-scalability-roadmap.md). Reserving
headroom for RT-UI unconditionally sacrifices some throughput available to background Agents on a
lightly loaded system in exchange for a latency guarantee that never degrades under load — a
deliberate reading of the [Golden Rule](01-vision-and-philosophy.md#2-the-golden-rule) as favoring
felt responsiveness over aggregate throughput.

## Testing Strategy

A synthetic workload generator mixes classical process load (CPU/RAM/storage bursts) with
AI-shaped load (inference bursts of varying token budgets and model tiers, context-window churn) to
regression-test fairness and RT-UI latency guarantees against the targets in
[36 — Performance Benchmarks](36-performance-benchmarks.md). Chaos tests kill and restart the
governor, the ledger, and individual dispatch paths mid-epoch to verify the recovery mechanisms
above rather than only steady-state behavior. Admission-control invariants (no dimension ever
oversubscribed, RT-UI headroom never breached by non-RT-UI tasks) are checked by property-based
testing across randomized `ResourceVector` and `TaskDescriptor` sequences. The full hardware matrix
from Raspberry Pi-class SBCs to multi-GPU enterprise clusters, per
[37 — Scalability Roadmap](37-scalability-roadmap.md), is run against the same test suite to verify
that the *algorithm* — not just its tuned constants — holds across three orders of magnitude of
available capacity.

---
*Next: [05 — Intent Engine](05-intent-engine.md).*
