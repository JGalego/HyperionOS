//! `hyperion-scheduler`'s own named "distributed offload" gap, closed end to end: a real
//! `Scheduler` wired with a real `SchedulerOffloadBridge` over a real `FederationHub` actually
//! completes a `SchedClass::BatchDistributable` task that doesn't fit locally by dispatching it
//! to a real peer device -- not just a unit test of either half in isolation.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_federation::{FederationHub, FederationTrustTier, SchedulerOffloadBridge};
use hyperion_scheduler::{
    IntentId, ResourceDimension, ResourceLedger, ResourceVector, SchedClass, Scheduler,
    TaskDescriptor, TaskId,
};

/// `SchedulerOffloadBridge` calls `FederationHub::dispatch_offload` with a real wall-clock `now`
/// (see that type's own doc comment on why -- `Scheduler::schedule_epoch` takes no `now`
/// parameter for any caller to thread one through), so a real ledger publication in these tests
/// must be published against real wall-clock time too, not a synthetic timestamp, to stay live
/// once the bridge actually checks it.
fn real_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn ample_ledger() -> ResourceVector {
    ResourceVector {
        cpu_shares: 1000,
        ram_mb: 8192,
        gpu_shares: 100,
        vram_mb: 8192,
        storage_iops: 1000,
        network_bw_kbps: 100_000,
        inference_tokens_per_sec: 1000,
        context_window_slots: 1000,
        battery_budget_mw: 100_000,
    }
}

#[test]
fn a_batch_task_that_does_not_fit_locally_completes_on_a_real_peer_device() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let monitor = Arc::new(monitor);

    let hub = Arc::new(FederationHub::new());
    hub.join_device(&monitor, &root, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    hub.publish_ledger(1, ample_ledger(), 20, real_now(), 60)
        .unwrap();

    let bridge = Arc::new(SchedulerOffloadBridge::new(hub.clone(), monitor.clone()));
    let mut sched = Scheduler::new().with_offload_trigger(bridge);
    // Deliberately tiny local capacity -- the task below can never fit locally, forcing the
    // real offload path to be the only way it ever completes.
    sched.register_resource_provider(ResourceLedger::new(ResourceDimension::Cpu, 1, 0));

    let ticket = sched
        .submit_task(
            &monitor,
            TaskDescriptor {
                id: TaskId(1),
                owner_intent: IntentId(77),
                owner_agent: None,
                class: SchedClass::BatchDistributable,
                deadline: None,
                priority_weight: 0.5,
                request: ResourceVector {
                    cpu_shares: 10,
                    ram_mb: 128,
                    ..Default::default()
                },
                cap_token: root.clone(),
                capability_ref: Some("web.search".to_string()),
                args: serde_json::json!({"query": "hyperion os"}),
            },
        )
        .unwrap();

    let report = sched.schedule_epoch();
    let rationale = report.into_iter().find(|r| r.ticket == ticket).unwrap();

    assert!(
        rationale.admitted,
        "a real offload completion must be reported as admitted, got: {rationale:?}"
    );
    assert!(rationale.note.contains("federation offload"));
    let result = rationale
        .offload_result
        .expect("a completed offload must carry the real peer device's result");
    assert!(
        result["results"][0]
            .as_str()
            .unwrap()
            .contains("hyperion os"),
        "expected the real dispatched result from the peer device, got: {result:?}"
    );

    // A real, genuine correlation to the Intent that triggered this offload -- not a sentinel.
    let records = hub.trace_intent(&monitor, &root, 77).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].control_state,
        hyperion_explainability::ControlState::Completed
    );
}

#[test]
fn an_infeasible_offload_still_ages_and_requeues_instead_of_panicking_or_hanging() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let monitor = Arc::new(monitor);

    // No devices ever joined -- every real offload attempt is infeasible.
    let hub = Arc::new(FederationHub::new());
    let bridge = Arc::new(SchedulerOffloadBridge::new(hub, monitor.clone()));
    let mut sched = Scheduler::new().with_offload_trigger(bridge);
    sched.register_resource_provider(ResourceLedger::new(ResourceDimension::Cpu, 1, 0));

    let ticket = sched
        .submit_task(
            &monitor,
            TaskDescriptor {
                id: TaskId(1),
                owner_intent: IntentId(1),
                owner_agent: None,
                class: SchedClass::BatchDistributable,
                deadline: None,
                priority_weight: 0.5,
                request: ResourceVector {
                    cpu_shares: 10,
                    ..Default::default()
                },
                cap_token: root,
                capability_ref: Some("web.search".to_string()),
                args: serde_json::json!({"query": "x"}),
            },
        )
        .unwrap();

    let report = sched.schedule_epoch();
    let rationale = report.into_iter().find(|r| r.ticket == ticket).unwrap();
    assert!(!rationale.admitted);
    assert!(rationale.note.contains("aged and requeued"));
    assert_eq!(rationale.offload_result, None);
}
