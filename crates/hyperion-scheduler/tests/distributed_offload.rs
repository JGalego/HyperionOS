//! This crate's own named "distributed offload" gap, made real via dependency injection: a wired
//! `OffloadTrigger` is asked to place a `SchedClass::BatchDistributable` task on a real peer
//! device before `schedule_epoch` finally falls back to aging and requeuing. The real adapter
//! over `hyperion_federation::FederationHub::dispatch_offload` lives in that crate (see its own
//! `SchedulerOffloadBridge`); this file exercises the scheduler-side contract with a small, real,
//! in-process `OffloadTrigger` implementation instead.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_scheduler::{
    IntentId, OffloadTrigger, ResourceDimension, ResourceLedger, ResourceVector, SchedClass,
    Scheduler, TaskDescriptor, TaskId,
};

/// A real, deterministic `OffloadTrigger`: succeeds for any task whose `capability_ref` matches
/// `accepts`, recording every task it was actually asked about so a test can assert it was (or
/// wasn't) really invoked.
struct FakeTrigger {
    accepts: &'static str,
    calls: AtomicUsize,
}

impl FakeTrigger {
    fn new(accepts: &'static str) -> Self {
        FakeTrigger {
            accepts,
            calls: AtomicUsize::new(0),
        }
    }
}

impl OffloadTrigger for FakeTrigger {
    fn try_offload(&self, task: &TaskDescriptor) -> Option<serde_json::Value> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if task.capability_ref.as_deref() == Some(self.accepts) {
            Some(serde_json::json!({"real_result": "computed on a real peer device"}))
        } else {
            None
        }
    }
}

fn task(
    id: u64,
    class: SchedClass,
    cpu: u32,
    capability_ref: Option<&str>,
    cap_token: hyperion_capability::CapabilityToken,
) -> TaskDescriptor {
    TaskDescriptor {
        id: TaskId(id),
        owner_intent: IntentId(id),
        owner_agent: None,
        class,
        deadline: None,
        priority_weight: 1.0,
        request: ResourceVector {
            cpu_shares: cpu,
            ..Default::default()
        },
        cap_token,
        capability_ref: capability_ref.map(str::to_string),
        args: serde_json::json!({"real": "payload"}),
    }
}

fn oversubscribed_scheduler(trigger: Arc<dyn OffloadTrigger>) -> Scheduler {
    let mut sched = Scheduler::new().with_offload_trigger(trigger);
    // Capacity of 1 CPU share -- any task requesting more than that never fits locally.
    sched.register_resource_provider(ResourceLedger::new(ResourceDimension::Cpu, 1, 0));
    sched
}

#[test]
fn a_batch_distributable_task_that_does_not_fit_locally_completes_via_a_real_offload_trigger() {
    let mut monitor = CapabilityMonitor::new();
    let cap = monitor.mint_root(RightsMask::EXEC, TrustBoundaryId(1), None);
    let trigger = Arc::new(FakeTrigger::new("batch.analyze"));
    let mut sched = oversubscribed_scheduler(trigger.clone());

    let ticket = sched
        .submit_task(
            &monitor,
            task(
                1,
                SchedClass::BatchDistributable,
                10,
                Some("batch.analyze"),
                cap,
            ),
        )
        .unwrap();
    let report = sched.schedule_epoch();
    let rationale = report.into_iter().find(|r| r.ticket == ticket).unwrap();

    assert!(
        rationale.admitted,
        "a real offload completion must be reported as admitted"
    );
    assert_eq!(
        rationale.offload_result,
        Some(serde_json::json!({"real_result": "computed on a real peer device"}))
    );
    assert_eq!(trigger.calls.load(Ordering::Relaxed), 1);
}

#[test]
fn a_non_distributable_task_that_does_not_fit_never_reaches_the_offload_trigger() {
    let mut monitor = CapabilityMonitor::new();
    let cap = monitor.mint_root(RightsMask::EXEC, TrustBoundaryId(1), None);
    let trigger = Arc::new(FakeTrigger::new("batch.analyze"));
    let mut sched = oversubscribed_scheduler(trigger.clone());

    sched
        .submit_task(
            &monitor,
            task(
                1,
                SchedClass::InteractiveAgent,
                10,
                Some("batch.analyze"),
                cap,
            ),
        )
        .unwrap();
    sched.schedule_epoch();

    assert_eq!(
        trigger.calls.load(Ordering::Relaxed),
        0,
        "SchedClass::InteractiveAgent must never be offered for offload, by design"
    );
}

#[test]
fn a_task_with_no_capability_ref_ages_and_requeues_without_ever_calling_the_trigger() {
    let mut monitor = CapabilityMonitor::new();
    let cap = monitor.mint_root(RightsMask::EXEC, TrustBoundaryId(1), None);
    let trigger = Arc::new(FakeTrigger::new("batch.analyze"));
    let mut sched = oversubscribed_scheduler(trigger.clone());

    let ticket = sched
        .submit_task(
            &monitor,
            task(1, SchedClass::BatchDistributable, 10, None, cap),
        )
        .unwrap();
    let report = sched.schedule_epoch();
    let rationale = report.into_iter().find(|r| r.ticket == ticket).unwrap();

    assert!(!rationale.admitted);
    assert!(rationale.note.contains("aged and requeued"));
    assert_eq!(trigger.calls.load(Ordering::Relaxed), 0);
}

#[test]
fn a_trigger_that_cannot_place_the_task_still_ages_and_requeues() {
    let mut monitor = CapabilityMonitor::new();
    let cap = monitor.mint_root(RightsMask::EXEC, TrustBoundaryId(1), None);
    // Wired, but this trigger only ever accepts a different capability.
    let trigger = Arc::new(FakeTrigger::new("some.other.capability"));
    let mut sched = oversubscribed_scheduler(trigger.clone());

    let ticket = sched
        .submit_task(
            &monitor,
            task(
                1,
                SchedClass::BatchDistributable,
                10,
                Some("batch.analyze"),
                cap,
            ),
        )
        .unwrap();
    let report = sched.schedule_epoch();
    let rationale = report.into_iter().find(|r| r.ticket == ticket).unwrap();

    assert!(!rationale.admitted);
    assert!(rationale.note.contains("aged and requeued"));
    assert_eq!(rationale.offload_result, None);
    assert_eq!(trigger.calls.load(Ordering::Relaxed), 1);
}

#[test]
fn with_no_trigger_wired_a_batch_distributable_task_ages_and_requeues_exactly_as_before() {
    let mut monitor = CapabilityMonitor::new();
    let cap = monitor.mint_root(RightsMask::EXEC, TrustBoundaryId(1), None);
    let mut sched = Scheduler::new();
    sched.register_resource_provider(ResourceLedger::new(ResourceDimension::Cpu, 1, 0));

    let ticket = sched
        .submit_task(
            &monitor,
            task(
                1,
                SchedClass::BatchDistributable,
                10,
                Some("batch.analyze"),
                cap,
            ),
        )
        .unwrap();
    let report = sched.schedule_epoch();
    let rationale = report.into_iter().find(|r| r.ticket == ticket).unwrap();

    assert!(!rationale.admitted);
    assert!(rationale.note.contains("aged and requeued"));
}
