//! docs/23-multi-model-orchestration.md's own previously-named gap: `RolloutStage::Canary` was
//! "tracked and lightly discounted in scoring, but no random sampling actually splits live
//! traffic by percentage." `RolloutStage::Canary(f32)` now carries a real traffic-percentage
//! payload, and `ModelRouter::route` really samples it deterministically per real call.

use std::collections::HashMap;
use std::sync::Arc;

use hyperion_ai_runtime::{LocalAiRuntime, MockBackend};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_model_router::{
    CapabilityInvocation, ConsequenceTier, CostModel, ExclusionReason, ImplId, ImplKind,
    ImplementationDescriptor, ModelRouter, PrivacyTier, RolloutStage, UrgencyClass,
};

fn router() -> ModelRouter {
    ModelRouter::new(Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000)))
}

fn monitor_and_token() -> (CapabilityMonitor, hyperion_capability::CapabilityToken) {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    (monitor, token)
}

fn descriptor(impl_id: u64, capability_id: &str) -> ImplementationDescriptor {
    let mut quality_profile = HashMap::new();
    quality_profile.insert(capability_id.to_string(), 0.7);
    ImplementationDescriptor {
        impl_id: ImplId(impl_id),
        capability_id: capability_id.to_string(),
        kind: ImplKind::NativeBinary,
        model_class: None,
        privacy_tier: PrivacyTier::Local,
        cost_model: CostModel::Free,
        quality_profile,
        declared_latency_ms: 100,
        // Real registration always enters at Shadow (docs/23 §Architecture) -- the caller must
        // separately promote via `set_rollout_stage`, matching every real caller's own two-step
        // registration+promotion flow.
        rollout_stage: RolloutStage::Shadow,
        resource_cost: None,
    }
}

/// Registers `impl_id`, then really promotes it to `stage` via the real, separate
/// `set_rollout_stage` call -- matching this crate's own "registration and promotion are
/// deliberately separate decisions" convention.
fn register_at_stage(
    router: &ModelRouter,
    monitor: &CapabilityMonitor,
    token: &hyperion_capability::CapabilityToken,
    impl_id: u64,
    capability_id: &str,
    stage: RolloutStage,
) {
    router
        .register_implementation(monitor, token, descriptor(impl_id, capability_id))
        .unwrap();
    router
        .set_rollout_stage(monitor, token, ImplId(impl_id), stage)
        .unwrap();
}

fn invocation(capability_id: &str) -> CapabilityInvocation {
    CapabilityInvocation {
        capability_id: capability_id.to_string(),
        urgency_class: UrgencyClass::Interactive,
        consequence_tier: ConsequenceTier::Routine,
        quality_floor: None,
        latency_budget_ms: 5_000,
        cloud_consent: false,
    }
}

#[test]
fn a_zero_percent_canary_is_never_sampled_in_and_falls_back_to_ga() {
    let router = router();
    let (monitor, token) = monitor_and_token();
    register_at_stage(
        &router,
        &monitor,
        &token,
        1,
        "document.draft",
        RolloutStage::Canary(0.0),
    );
    register_at_stage(
        &router,
        &monitor,
        &token,
        2,
        "document.draft",
        RolloutStage::Ga,
    );

    for _ in 0..20 {
        let decision = router.route(&invocation("document.draft"));
        assert_eq!(
            decision.chosen,
            Some(ImplId(2)),
            "a 0% canary must never be chosen; the GA candidate is the real safety net"
        );
        assert!(decision
            .rationale
            .candidates_excluded
            .iter()
            .any(|(id, reason)| *id == ImplId(1) && *reason == ExclusionReason::CanaryNotSampled));
    }
}

#[test]
fn a_hundred_percent_canary_is_always_sampled_in() {
    let router = router();
    let (monitor, token) = monitor_and_token();
    register_at_stage(
        &router,
        &monitor,
        &token,
        1,
        "document.draft",
        RolloutStage::Canary(1.0),
    );

    for _ in 0..20 {
        let decision = router.route(&invocation("document.draft"));
        assert!(
            decision
                .rationale
                .candidates_considered
                .iter()
                .any(|(id, _)| *id == ImplId(1)),
            "a 100% canary must be a real candidate on every real call"
        );
        assert!(decision.rationale.candidates_excluded.is_empty());
    }
}

#[test]
fn a_partial_canary_percentage_really_splits_live_traffic_over_many_real_calls() {
    let router = router();
    let (monitor, token) = monitor_and_token();
    register_at_stage(
        &router,
        &monitor,
        &token,
        1,
        "document.draft",
        RolloutStage::Canary(0.3),
    );
    register_at_stage(
        &router,
        &monitor,
        &token,
        2,
        "document.draft",
        RolloutStage::Ga,
    );

    let total = 2_000;
    let sampled_in = (0..total)
        .filter(|_| {
            router
                .route(&invocation("document.draft"))
                .rationale
                .candidates_considered
                .iter()
                .any(|(id, _)| *id == ImplId(1))
        })
        .count();

    let rate = sampled_in as f64 / total as f64;
    assert!(
        (0.20..=0.40).contains(&rate),
        "expected a real sampling rate near 30% over {total} real calls, got {rate:.3}"
    );
}

#[test]
fn two_independent_canary_candidates_sample_independently_not_in_lockstep() {
    let router = router();
    let (monitor, token) = monitor_and_token();
    register_at_stage(
        &router,
        &monitor,
        &token,
        1,
        "document.draft",
        RolloutStage::Canary(0.5),
    );
    register_at_stage(
        &router,
        &monitor,
        &token,
        2,
        "document.draft",
        RolloutStage::Canary(0.5),
    );
    register_at_stage(
        &router,
        &monitor,
        &token,
        3,
        "document.draft",
        RolloutStage::Ga,
    );

    let mut both_in = 0;
    let mut only_one_in = 0;
    let total = 500;
    for _ in 0..total {
        let decision = router.route(&invocation("document.draft"));
        let considered: Vec<ImplId> = decision
            .rationale
            .candidates_considered
            .iter()
            .map(|(id, _)| *id)
            .collect();
        let one_in = considered.contains(&ImplId(1));
        let two_in = considered.contains(&ImplId(2));
        if one_in && two_in {
            both_in += 1;
        } else if one_in != two_in {
            only_one_in += 1;
        }
    }

    assert!(
        only_one_in > 0,
        "two independently-sampled 50% canaries must sometimes disagree on the same real call, \
         not always move together (both_in={both_in}, only_one_in={only_one_in})"
    );
}

#[test]
fn a_sampled_in_canary_still_scores_with_the_existing_availability_discount() {
    let router = router();
    let (monitor, token) = monitor_and_token();
    register_at_stage(
        &router,
        &monitor,
        &token,
        1,
        "document.draft",
        RolloutStage::Canary(1.0),
    );

    let decision = router.route(&invocation("document.draft"));
    let (_, score) = decision
        .rationale
        .candidates_considered
        .iter()
        .find(|(id, _)| *id == ImplId(1))
        .unwrap();
    assert_eq!(
        score.availability_fit, 0.8,
        "a sampled-in canary must still carry the real, modest availability discount versus GA"
    );
}
