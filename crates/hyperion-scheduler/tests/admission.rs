use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_scheduler::{
    IntentId, ResourceDimension, ResourceLedger, ResourceVector, SchedClass, SchedError, Scheduler,
    TaskDescriptor, TaskId,
};

fn cpu_ledger(capacity: u32, reserved: u32) -> ResourceLedger {
    ResourceLedger::new(ResourceDimension::Cpu, capacity, reserved)
}

fn task(
    id: u64,
    owner: u64,
    class: SchedClass,
    cpu: u32,
    cap_token: hyperion_capability::CapabilityToken,
) -> TaskDescriptor {
    TaskDescriptor {
        id: TaskId(id),
        owner_intent: IntentId(owner),
        owner_agent: None,
        class,
        deadline: None,
        priority_weight: 1.0,
        request: ResourceVector {
            cpu_shares: cpu,
            ..Default::default()
        },
        cap_token,
        capability_ref: None,
        args: serde_json::Value::Null,
    }
}

#[test]
fn admits_a_task_that_fits_and_records_the_allocation() {
    let mut monitor = CapabilityMonitor::new();
    let cap = monitor.mint_root(RightsMask::EXEC, TrustBoundaryId(1), None);

    let mut sched = Scheduler::new();
    sched.register_resource_provider(cpu_ledger(100, 0));
    let t = task(1, 1, SchedClass::BackgroundAgent, 40, cap);
    sched.submit_task(&monitor, t).unwrap();

    let report = sched.schedule_epoch();
    assert_eq!(report.len(), 1);
    assert!(report[0].admitted);
    assert_eq!(
        sched
            .query_ledger(ResourceDimension::Cpu)
            .unwrap()
            .allocated,
        40
    );
}

#[test]
fn refuses_submission_without_a_live_capability() {
    let mut monitor = CapabilityMonitor::new();
    let cap = monitor.mint_root(RightsMask::EXEC, TrustBoundaryId(1), None);
    monitor.cap_revoke(&cap);

    let mut sched = Scheduler::new();
    sched.register_resource_provider(cpu_ledger(100, 0));
    let t = task(1, 1, SchedClass::BackgroundAgent, 10, cap);
    assert_eq!(
        sched.submit_task(&monitor, t).unwrap_err(),
        SchedError::Unauthorized
    );
}

#[test]
fn oversubscribed_dimension_defers_the_excess_task_via_aging() {
    let mut monitor = CapabilityMonitor::new();
    let mut sched = Scheduler::new();
    sched.register_resource_provider(cpu_ledger(100, 0));

    let cap_a = monitor.mint_root(RightsMask::EXEC, TrustBoundaryId(1), None);
    let cap_b = monitor.mint_root(RightsMask::EXEC, TrustBoundaryId(2), None);
    sched
        .submit_task(&monitor, task(1, 1, SchedClass::BackgroundAgent, 70, cap_a))
        .unwrap();
    sched
        .submit_task(&monitor, task(2, 2, SchedClass::BackgroundAgent, 70, cap_b))
        .unwrap();

    let report = sched.schedule_epoch();
    let admitted_count = report.iter().filter(|r| r.admitted).count();
    assert_eq!(
        admitted_count, 1,
        "capacity 100 cannot admit both 70-unit tasks in one epoch"
    );
    assert!(
        sched
            .query_ledger(ResourceDimension::Cpu)
            .unwrap()
            .allocated
            <= 100
    );
}

#[test]
fn realtime_ui_dispatches_by_earliest_deadline_against_reserved_headroom() {
    use std::time::{Duration, Instant};

    // Reserved headroom is a *floor* guaranteed to RealTimeUI, not a ceiling
    // on it — an RT-UI task may draw on the full ledger capacity. To force
    // genuine contention between two RT-UI tasks, capacity itself (not just
    // the reserved slice) must be too small for both.
    let mut monitor = CapabilityMonitor::new();
    let mut sched = Scheduler::new();
    sched.register_resource_provider(cpu_ledger(10, 10));

    let cap_a = monitor.mint_root(RightsMask::EXEC, TrustBoundaryId(1), None);
    let cap_b = monitor.mint_root(RightsMask::EXEC, TrustBoundaryId(2), None);

    let now = Instant::now();
    let mut earlier = task(1, 1, SchedClass::RealTimeUI, 6, cap_a);
    earlier.deadline = Some(now + Duration::from_millis(1));
    let mut later = task(2, 2, SchedClass::RealTimeUI, 6, cap_b);
    later.deadline = Some(now + Duration::from_millis(50));

    // Submit out of deadline order to prove EDF sorts them, not insertion order.
    sched.submit_task(&monitor, later.clone()).unwrap();
    sched.submit_task(&monitor, earlier.clone()).unwrap();

    let report = sched.schedule_epoch();
    let earlier_result = report.iter().find(|r| r.ticket == earlier.id).unwrap();
    let later_result = report.iter().find(|r| r.ticket == later.id).unwrap();

    assert!(
        earlier_result.admitted,
        "the earlier deadline must be served first"
    );
    assert!(
        !later_result.admitted,
        "only 10 reserved units exist; the later deadline can't also fit"
    );
}
