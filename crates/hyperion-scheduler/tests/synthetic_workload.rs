//! docs/41-implementation-phases.md's Phase 1 exit criterion: "the scheduler
//! admits and fairly shares CPU/GPU/RAM/battery load across synthetic
//! Real-Time-UI, Interactive, and Background classes per 04's algorithm."
//! Mirrors docs/04-scheduler.md §Testing Strategy's own prescription: "A
//! synthetic workload generator mixes classical process load... with
//! AI-shaped load... to regression-test fairness and RT-UI latency
//! guarantees."

use std::time::{Duration, Instant};

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_scheduler::{
    AgentId, IntentId, ResourceDimension, ResourceLedger, ResourceVector, SchedClass, Scheduler,
    TaskDescriptor, TaskId,
};

const INTERACTIVE_OWNER: u64 = 1;
const BACKGROUND_OWNER: u64 = 2;
const CONTENDED_DIMS: [ResourceDimension; 4] = [
    ResourceDimension::Cpu,
    ResourceDimension::Gpu,
    ResourceDimension::Ram,
    ResourceDimension::Battery,
];

fn register_ledgers(sched: &mut Scheduler) {
    // Reserved headroom is a small but nonzero slice of every dimension the
    // exit criterion names, so RealTimeUI's independence from Interactive/
    // Background load is a real property of these numbers, not a vacuous one.
    sched.register_resource_provider(ResourceLedger::new(ResourceDimension::Cpu, 100, 20));
    sched.register_resource_provider(ResourceLedger::new(ResourceDimension::Gpu, 50, 10));
    sched.register_resource_provider(ResourceLedger::new(ResourceDimension::Ram, 1000, 100));
    sched.register_resource_provider(ResourceLedger::new(ResourceDimension::Battery, 1000, 50));
}

fn realtime_ui_task(id: u64, cap: CapabilityToken) -> TaskDescriptor {
    TaskDescriptor {
        id: TaskId(id),
        owner_intent: IntentId(1000 + id),
        owner_agent: None,
        class: SchedClass::RealTimeUI,
        deadline: Some(Instant::now() + Duration::from_millis(5)),
        priority_weight: 1.0,
        request: ResourceVector {
            cpu_shares: 2,
            gpu_shares: 1,
            ram_mb: 10,
            battery_budget_mw: 5,
            ..Default::default()
        },
        cap_token: cap,
    }
}

/// One unit of Interactive/Background load. Splitting each owner's demand
/// into several same-size units (rather than one big task) is what lets DRF
/// interleave admissions between them proportionally to weight instead of
/// an all-or-nothing per-epoch contest — the same mechanism
/// tests/fairness.rs exercises, applied here across four dimensions and
/// three classes at once instead of one dimension and two owners.
fn agent_task(
    id: u64,
    owner: u64,
    class: SchedClass,
    weight: f32,
    cap: CapabilityToken,
) -> TaskDescriptor {
    TaskDescriptor {
        id: TaskId(id),
        owner_intent: IntentId(owner),
        owner_agent: Some(AgentId(owner)),
        class,
        deadline: None,
        priority_weight: weight,
        request: ResourceVector {
            cpu_shares: 10,
            gpu_shares: 5,
            ram_mb: 50,
            battery_budget_mw: 25,
            ..Default::default()
        },
        cap_token: cap,
    }
}

#[test]
fn synthetic_workload_keeps_realtime_ui_independent_and_shares_the_rest_by_weight() {
    let mut monitor = CapabilityMonitor::new();
    let mut sched = Scheduler::new();
    register_ledgers(&mut sched);

    let rt_cap = monitor.mint_root(RightsMask::EXEC, TrustBoundaryId(1), None);
    let interactive_cap = monitor.mint_root(RightsMask::EXEC, TrustBoundaryId(2), None);
    let background_cap = monitor.mint_root(RightsMask::EXEC, TrustBoundaryId(3), None);

    let mut next_id = 0u64;
    let mut realtime_submitted = 0u32;
    let mut realtime_admitted = 0u32;
    let mut interactive_admitted_units = 0u32;
    let mut background_admitted_units = 0u32;

    for _epoch in 0..25 {
        let mut rt_ids = Vec::new();
        for _ in 0..3 {
            next_id += 1;
            sched
                .submit_task(&monitor, realtime_ui_task(next_id, rt_cap.clone()))
                .unwrap();
            rt_ids.push(next_id);
            realtime_submitted += 1;
        }

        let mut interactive_ids = Vec::new();
        for _ in 0..6 {
            next_id += 1;
            sched
                .submit_task(
                    &monitor,
                    agent_task(
                        next_id,
                        INTERACTIVE_OWNER,
                        SchedClass::InteractiveAgent,
                        2.0,
                        interactive_cap.clone(),
                    ),
                )
                .unwrap();
            interactive_ids.push(next_id);
        }
        let mut background_ids = Vec::new();
        for _ in 0..6 {
            next_id += 1;
            sched
                .submit_task(
                    &monitor,
                    agent_task(
                        next_id,
                        BACKGROUND_OWNER,
                        SchedClass::BackgroundAgent,
                        1.0,
                        background_cap.clone(),
                    ),
                )
                .unwrap();
            background_ids.push(next_id);
        }

        let report = sched.schedule_epoch();

        for r in &report {
            if rt_ids.contains(&r.ticket.0) {
                if r.admitted {
                    realtime_admitted += 1;
                }
            } else if r.admitted && interactive_ids.contains(&r.ticket.0) {
                interactive_admitted_units += 1;
            } else if r.admitted && background_ids.contains(&r.ticket.0) {
                background_admitted_units += 1;
            }
        }

        // Admission-control invariant, checked every epoch: no dimension the
        // exit criterion names is ever oversubscribed.
        for dim in CONTENDED_DIMS {
            let ledger = sched.query_ledger(dim).unwrap();
            assert!(
                ledger.allocated <= ledger.capacity,
                "{dim:?} oversubscribed: {} allocated > {} capacity",
                ledger.allocated,
                ledger.capacity
            );
        }

        // Release admitted work and drop anything that didn't fit, so each
        // epoch is an independent, isolated contest — same rationale as
        // tests/fairness.rs's repeated-round check.
        for r in &report {
            if r.admitted {
                let _ = sched.complete(r.ticket);
            } else {
                let _ = sched.cancel(r.ticket);
            }
        }
    }

    assert_eq!(
        realtime_admitted, realtime_submitted,
        "RealTimeUI must be admitted every epoch regardless of Interactive/Background load on \
         the same dimensions — reserved headroom is supposed to make compositor/input latency \
         provably independent of background Agent load (docs/04 §Motivation)"
    );

    assert!(
        interactive_admitted_units > 0 && background_admitted_units > 0,
        "both classes must receive *some* share of contested capacity — a weighted scheduler \
         still isn't 'fairly sharing' if one class is unconditionally starved: \
         interactive={interactive_admitted_units}, background={background_admitted_units}"
    );
    assert!(
        interactive_admitted_units > background_admitted_units,
        "InteractiveAgent's higher priority_weight (\"user is waiting\", docs/04 §Data \
         Structures) must win it more of the contested capacity than BackgroundAgent's lower \
         base weight: interactive={interactive_admitted_units}, background={background_admitted_units}"
    );
}
