//! Regression test for the fairness bug docs/04-scheduler.md's DRF design
//! specifically exists to prevent: an owner submitting many small tasks
//! must not gain unfair share over one submitting few large ones. A
//! scheduler that ranked by a task's own request size (rather than its
//! owner's *cumulative* currently-held allocation) would let "Splitter"
//! monopolize contested capacity simply because each of her individual
//! requests looks cheap in isolation, starving "Chunky" even though their
//! total demand is comparable.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_scheduler::{
    IntentId, ResourceDimension, ResourceLedger, ResourceVector, SchedClass, Scheduler,
    TaskDescriptor, TaskId,
};

const SPLITTER: u64 = 1;
const CHUNKY: u64 = 2;

fn cpu_task(
    id: u64,
    owner: u64,
    cpu: u32,
    cap_token: hyperion_capability::CapabilityToken,
) -> TaskDescriptor {
    TaskDescriptor {
        id: TaskId(id),
        owner_intent: IntentId(owner),
        owner_agent: None,
        class: SchedClass::BackgroundAgent,
        deadline: None,
        priority_weight: 1.0, // equal weight: any share difference must come from request shape, not priority
        request: ResourceVector {
            cpu_shares: cpu,
            ..Default::default()
        },
        cap_token,
        capability_ref: None,
    }
}

#[test]
fn splitting_demand_into_many_small_tasks_gains_no_unfair_share() {
    let mut monitor = CapabilityMonitor::new();
    let mut sched = Scheduler::new();
    sched.register_resource_provider(ResourceLedger::new(ResourceDimension::Cpu, 100, 0));

    let splitter_cap = monitor.mint_root(RightsMask::EXEC, TrustBoundaryId(1), None);
    let chunky_cap = monitor.mint_root(RightsMask::EXEC, TrustBoundaryId(2), None);

    // Splitter's total demand (90) alone would fill the ledger if admitted
    // uninterrupted; Chunky's single request (50) is smaller in isolation
    // but represents an equal-weight owner's fair half of contested capacity.
    let mut next_id = 0u64;
    for _ in 0..90 {
        next_id += 1;
        sched
            .submit_task(
                &monitor,
                cpu_task(next_id, SPLITTER, 1, splitter_cap.clone()),
            )
            .unwrap();
    }
    next_id += 1;
    sched
        .submit_task(&monitor, cpu_task(next_id, CHUNKY, 50, chunky_cap))
        .unwrap();

    let report = sched.schedule_epoch();

    let chunky_admitted = report
        .iter()
        .find(|r| r.ticket == TaskId(next_id))
        .unwrap()
        .admitted;
    assert!(
        chunky_admitted,
        "a per-task-ranked (non-cumulative) scheduler would starve Chunky indefinitely, \
         since every one of Splitter's individual 1-unit requests looks cheaper than \
         Chunky's 50-unit request in isolation"
    );

    let splitter_admitted_units = report
        .iter()
        .filter(|r| r.ticket != TaskId(next_id) && r.admitted)
        .count() as u32;

    // Capacity is exactly 100 and both owners have equal weight: a fair
    // split of contested capacity is 50/50, regardless of how Splitter
    // chose to slice her demand into 90 separate requests.
    assert_eq!(
        splitter_admitted_units, 50,
        "equal-weight owners must receive an equal share of contested capacity"
    );
    assert_eq!(
        sched
            .query_ledger(ResourceDimension::Cpu)
            .unwrap()
            .allocated,
        100
    );
}

#[test]
fn strategy_proofness_holds_across_repeated_epochs_with_reclaim() {
    // A second, longer-running check of the same property: run several
    // rounds where each round's admitted work "completes" before the next
    // round submits more, and confirm neither owner's cumulative admitted
    // total diverges from the other's over time.
    let mut monitor = CapabilityMonitor::new();
    let mut sched = Scheduler::new();
    sched.register_resource_provider(ResourceLedger::new(ResourceDimension::Cpu, 20, 0));

    let splitter_cap = monitor.mint_root(RightsMask::EXEC, TrustBoundaryId(1), None);
    let chunky_cap = monitor.mint_root(RightsMask::EXEC, TrustBoundaryId(2), None);

    let mut splitter_total = 0u32;
    let mut chunky_total = 0u32;
    let mut next_id = 0u64;

    for _round in 0..10 {
        let mut round_ids = Vec::new();
        for _ in 0..15 {
            next_id += 1;
            sched
                .submit_task(
                    &monitor,
                    cpu_task(next_id, SPLITTER, 1, splitter_cap.clone()),
                )
                .unwrap();
            round_ids.push(next_id);
        }
        next_id += 1;
        sched
            .submit_task(&monitor, cpu_task(next_id, CHUNKY, 10, chunky_cap.clone()))
            .unwrap();
        let chunky_id = next_id;

        let report = sched.schedule_epoch();
        for r in &report {
            if !r.admitted {
                continue;
            }
            if r.ticket == TaskId(chunky_id) {
                chunky_total += 10;
            } else if round_ids.contains(&r.ticket.0) {
                splitter_total += 1;
            }
        }
        // Release everything admitted this round, and drop anything that
        // didn't fit rather than letting it snowball into the next round's
        // backlog — each round is meant to isolate the same fresh-demand
        // contest, not accumulate an ever-growing queue.
        for r in &report {
            if r.admitted {
                let _ = sched.complete(r.ticket);
            } else {
                let _ = sched.cancel(r.ticket);
            }
        }
    }

    let diff = (splitter_total as i64 - chunky_total as i64).abs();
    assert!(
        diff <= 10,
        "cumulative shares diverged: splitter={splitter_total}, chunky={chunky_total}"
    );
}
