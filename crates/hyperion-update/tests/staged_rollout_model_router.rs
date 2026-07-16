//! `hyperion-model-router`'s own previously-named gap: "*deciding* what percentage to declare
//! and when to ratchet it up over a real rollout's lifetime remains [32 — Update System]'s own
//! job." `UpdateOrchestrator::apply_update_with_rollout` is this crate really being that caller —
//! each real, health-gated stage genuinely drives `ModelRouter::set_rollout_stage`.

use std::sync::Arc;

use hyperion_ai_runtime::{LocalAiRuntime, MockBackend};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_model_router::{
    CostModel, ImplId, ImplKind, ImplementationDescriptor, ModelRouter, PrivacyTier, RolloutStage,
};
use hyperion_recovery::RecoveryService;
use hyperion_update::{
    sign, CohortHealth, HealthThresholds, RolloutPolicy, UpdateError, UpdateManifest,
    UpdateOrchestrator, UpdateSubject,
};

fn setup() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    UpdateOrchestrator,
    Arc<ModelRouter>,
) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let recovery = Arc::new(RecoveryService::new(graph));
    let orchestrator = UpdateOrchestrator::new(recovery);
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));
    let router = Arc::new(ModelRouter::new(ai_runtime));
    (monitor, root, orchestrator, router)
}

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

fn healthy_thresholds() -> HealthThresholds {
    HealthThresholds {
        max_crash_rate: 0.01,
        max_latency_p99_ms: 500,
    }
}

fn manifest(keystore: &Keystore) -> UpdateManifest {
    let mut m = UpdateManifest {
        subject: UpdateSubject::Capability {
            id: "document.summarize".to_string(),
        },
        from_version: 0,
        to_version: 1,
        signature: None,
        touched_objects: vec![],
        rollout_policy: RolloutPolicy::default_schedule(healthy_thresholds()),
    };
    m.signature = Some(sign(&m, keystore));
    m
}

fn register_shadow_candidate(
    router: &ModelRouter,
    monitor: &CapabilityMonitor,
    token: &hyperion_capability::CapabilityToken,
    impl_id: ImplId,
) {
    router
        .register_implementation(
            monitor,
            token,
            ImplementationDescriptor {
                impl_id,
                capability_id: "document.summarize".to_string(),
                kind: ImplKind::CloudApi,
                model_class: None,
                privacy_tier: PrivacyTier::Local,
                cost_model: CostModel::Free,
                quality_profile: Default::default(),
                declared_latency_ms: 100,
                rollout_stage: RolloutStage::Shadow,
                resource_cost: None,
            },
        )
        .unwrap();
}

#[test]
fn a_healthy_rollout_really_advances_the_model_router_candidate_to_ga() {
    let (monitor, root, orchestrator, router) = setup();
    let (_dir, keystore) = keystore();
    let impl_id = ImplId(1);
    register_shadow_candidate(&router, &monitor, &root, impl_id);
    let m = manifest(&keystore);

    let version = orchestrator
        .apply_update_with_rollout(
            &monitor,
            &root,
            &m,
            true,
            1_000,
            |_percent| CohortHealth {
                crash_rate: 0.0,
                latency_p99_ms: 50,
            },
            &keystore.verifying_key(),
            &router,
            impl_id,
        )
        .unwrap();

    assert_eq!(version, 1);
    assert_eq!(
        router.descriptor(impl_id).unwrap().rollout_stage,
        RolloutStage::Ga,
        "every stage passed healthy, so the real candidate must reach full rollout"
    );
}

#[test]
fn each_healthy_stage_really_promotes_the_candidate_to_that_stages_own_real_percentage() {
    let (monitor, root, orchestrator, router) = setup();
    let (_dir, keystore) = keystore();
    let impl_id = ImplId(1);
    register_shadow_candidate(&router, &monitor, &root, impl_id);
    let m = manifest(&keystore);

    let mut observed_stages_before_each_call = Vec::new();
    orchestrator
        .apply_update_with_rollout(
            &monitor,
            &root,
            &m,
            true,
            1_000,
            |_percent| {
                // Whatever the *previous* stage's own real promotion left behind -- Shadow
                // before the very first stage, then each real Canary(pct) in turn.
                observed_stages_before_each_call
                    .push(router.descriptor(impl_id).unwrap().rollout_stage);
                CohortHealth {
                    crash_rate: 0.0,
                    latency_p99_ms: 50,
                }
            },
            &keystore.verifying_key(),
            &router,
            impl_id,
        )
        .unwrap();

    assert_eq!(
        observed_stages_before_each_call,
        vec![
            RolloutStage::Shadow,
            RolloutStage::Canary(0.01),
            RolloutStage::Canary(0.10),
            RolloutStage::Canary(0.50),
        ],
        "each real stage's health check must see exactly the previous stage's own real \
         promotion, proving this genuinely ratchets up rather than jumping straight to GA"
    );
}

#[test]
fn a_health_breach_demotes_the_real_candidate_back_to_shadow_not_leaving_it_live() {
    let (monitor, root, orchestrator, router) = setup();
    let (_dir, keystore) = keystore();
    let impl_id = ImplId(1);
    register_shadow_candidate(&router, &monitor, &root, impl_id);
    let mut m = manifest(&keystore);
    m.rollout_policy.auto_rollback_on_breach = false;
    m.signature = Some(sign(&m, &keystore));

    let mut calls = 0;
    let result = orchestrator.apply_update_with_rollout(
        &monitor,
        &root,
        &m,
        true,
        1_000,
        |_percent| {
            calls += 1;
            if calls == 1 {
                CohortHealth {
                    crash_rate: 0.0,
                    latency_p99_ms: 50,
                }
            } else {
                CohortHealth {
                    crash_rate: 1.0,
                    latency_p99_ms: 9_999,
                } // breach on the second stage
            }
        },
        &keystore.verifying_key(),
        &router,
        impl_id,
    );

    assert!(matches!(result, Err(UpdateError::RolloutHealthBreach)));
    assert_eq!(
        router.descriptor(impl_id).unwrap().rollout_stage,
        RolloutStage::Shadow,
        "a breached rollout must demote the real candidate back out of live traffic entirely, \
         not leave it stuck at whatever partial Canary(pct) it last reached"
    );
}
