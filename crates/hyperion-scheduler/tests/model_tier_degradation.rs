//! This crate's own named "model-tier degradation" gap, made real:
//! `Scheduler::schedule_epoch`'s non-admit branch now asks a wired `ModelRouter` for a cheaper
//! registered implementation of a task's own `capability_ref` before falling back to aging and
//! requeuing the original request.

use std::collections::HashMap;
use std::sync::Arc;

use hyperion_ai_runtime::{LocalAiRuntime, MockBackend};
use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_model_router::{
    CostModel, ImplId, ImplKind, ImplementationDescriptor, ModelRouter, PrivacyTier, ResourceCost,
    RolloutStage,
};
use hyperion_scheduler::{
    IntentId, ResourceDimension, ResourceLedger, ResourceVector, SchedClass, Scheduler,
    TaskDescriptor, TaskId,
};

fn monitor_and_token() -> (CapabilityMonitor, CapabilityToken) {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    (monitor, token)
}

fn register_ga(
    router: &ModelRouter,
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    impl_id: u64,
    capability_id: &str,
    resource_cost: Option<ResourceCost>,
) {
    router
        .register_implementation(
            monitor,
            token,
            ImplementationDescriptor {
                impl_id: ImplId(impl_id),
                capability_id: capability_id.to_string(),
                kind: ImplKind::CloudApi,
                model_class: None,
                privacy_tier: PrivacyTier::Local,
                cost_model: CostModel::Free,
                quality_profile: HashMap::new(),
                declared_latency_ms: 100,
                rollout_stage: RolloutStage::Shadow,
                resource_cost,
            },
        )
        .unwrap();
    router
        .set_rollout_stage(monitor, token, ImplId(impl_id), RolloutStage::Ga)
        .unwrap();
}

fn task(
    id: u64,
    cpu_shares: u32,
    capability_ref: Option<&str>,
    cap_token: CapabilityToken,
) -> TaskDescriptor {
    TaskDescriptor {
        id: TaskId(id),
        owner_intent: IntentId(id),
        owner_agent: None,
        class: SchedClass::BackgroundAgent,
        deadline: None,
        priority_weight: 1.0,
        request: ResourceVector {
            cpu_shares,
            ..Default::default()
        },
        cap_token,
        capability_ref: capability_ref.map(str::to_string),
        args: serde_json::Value::Null,
    }
}

#[test]
fn a_task_that_does_not_fit_is_admitted_at_a_cheaper_registered_implementation_instead() {
    let (monitor, token) = monitor_and_token();
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));
    let router = Arc::new(ModelRouter::new(ai_runtime));
    register_ga(
        &router,
        &monitor,
        &token,
        1,
        "heavy.task",
        Some(ResourceCost {
            cpu_shares: 10,
            ..Default::default()
        }),
    );

    let mut scheduler = Scheduler::new_with_model_router(router);
    scheduler.register_resource_provider(ResourceLedger::new(ResourceDimension::Cpu, 100, 0));

    let ticket = scheduler
        .submit_task(&monitor, task(1, 1_000, Some("heavy.task"), token.clone()))
        .unwrap();
    let report = scheduler.schedule_epoch();

    let rationale = report.into_iter().find(|r| r.ticket == ticket).unwrap();
    assert!(
        rationale.admitted,
        "a task whose own request doesn't fit must still be admitted at a cheaper registered \
         implementation, got: {rationale:?}"
    );
    assert!(
        rationale
            .note
            .contains("cheaper registered implementation 1"),
        "the rationale must name which real implementation it degraded to, got: {}",
        rationale.note
    );
}

#[test]
fn a_task_with_no_capability_ref_still_ages_and_requeues_when_it_does_not_fit() {
    let (monitor, token) = monitor_and_token();
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));
    let router = Arc::new(ModelRouter::new(ai_runtime));

    let mut scheduler = Scheduler::new_with_model_router(router);
    scheduler.register_resource_provider(ResourceLedger::new(ResourceDimension::Cpu, 100, 0));

    let ticket = scheduler
        .submit_task(&monitor, task(1, 1_000, None, token.clone()))
        .unwrap();
    let report = scheduler.schedule_epoch();

    let rationale = report.into_iter().find(|r| r.ticket == ticket).unwrap();
    assert!(
        !rationale.admitted,
        "with no capability_ref there is nothing to degrade to, so this must still age and \
         requeue exactly as before, got: {rationale:?}"
    );
    assert_eq!(rationale.note, "did not fit this epoch; aged and requeued");
}

#[test]
fn a_task_with_no_wired_model_router_still_ages_and_requeues_when_it_does_not_fit() {
    let (monitor, token) = monitor_and_token();
    let mut scheduler = Scheduler::new();
    scheduler.register_resource_provider(ResourceLedger::new(ResourceDimension::Cpu, 100, 0));

    let ticket = scheduler
        .submit_task(&monitor, task(1, 1_000, Some("heavy.task"), token.clone()))
        .unwrap();
    let report = scheduler.schedule_epoch();

    let rationale = report.into_iter().find(|r| r.ticket == ticket).unwrap();
    assert!(
        !rationale.admitted,
        "with no ModelRouter wired at all, degradation must never fire -- existing callers with \
         no router keep exactly their prior behavior, got: {rationale:?}"
    );
}
