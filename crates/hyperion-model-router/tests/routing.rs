//! docs/23-multi-model-orchestration.md's routing pipeline: the privacy
//! gate as a hard exclusion never rescuable by scoring, shadow-stage
//! exclusion from the returned decision, urgency/consequence-tier-driven
//! weight shifts, and the circuit breaker.

use std::collections::HashMap;
use std::sync::Arc;

use hyperion_ai_runtime::{LocalAiRuntime, MockBackend};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_model_router::{
    CapabilityInvocation, ConsequenceTier, CostModel, ImplId, ImplKind, ImplementationDescriptor,
    ModelRouter, PrivacyTier, RolloutStage, UrgencyClass,
};

fn router() -> ModelRouter {
    ModelRouter::new(Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000)))
}

fn monitor_and_token() -> (CapabilityMonitor, hyperion_capability::CapabilityToken) {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    (monitor, token)
}

fn cloud_descriptor(impl_id: u64, capability_id: &str, quality: f32) -> ImplementationDescriptor {
    let mut quality_profile = HashMap::new();
    quality_profile.insert(capability_id.to_string(), quality);
    ImplementationDescriptor {
        impl_id: ImplId(impl_id),
        capability_id: capability_id.to_string(),
        kind: ImplKind::CloudApi,
        model_class: None,
        privacy_tier: PrivacyTier::ConsentedCloud,
        cost_model: CostModel::PerCall(0.01),
        quality_profile,
        declared_latency_ms: 200,
        rollout_stage: RolloutStage::Ga,
        resource_cost: None,
    }
}

fn native_descriptor(
    impl_id: u64,
    capability_id: &str,
    latency_ms: u64,
    quality: f32,
    cost: CostModel,
) -> ImplementationDescriptor {
    let mut quality_profile = HashMap::new();
    quality_profile.insert(capability_id.to_string(), quality);
    ImplementationDescriptor {
        impl_id: ImplId(impl_id),
        capability_id: capability_id.to_string(),
        kind: ImplKind::NativeBinary,
        model_class: None,
        privacy_tier: PrivacyTier::Local,
        cost_model: cost,
        quality_profile,
        declared_latency_ms: latency_ms,
        rollout_stage: RolloutStage::Ga,
        resource_cost: None,
    }
}

fn invocation(
    capability_id: &str,
    urgency: UrgencyClass,
    consequence: ConsequenceTier,
    cloud_consent: bool,
) -> CapabilityInvocation {
    CapabilityInvocation {
        capability_id: capability_id.to_string(),
        urgency_class: urgency,
        consequence_tier: consequence,
        quality_floor: None,
        latency_budget_ms: 5_000,
        cloud_consent,
    }
}

#[test]
fn privacy_gate_excludes_cloud_candidate_even_with_perfect_scores_and_no_consent() {
    let (monitor, token) = monitor_and_token();
    let router = router();
    // A cloud candidate engineered to win on every other axis: perfect
    // quality, free, fast.
    let mut cloud = cloud_descriptor(1, "summarize", 1.0);
    cloud.cost_model = CostModel::Free;
    cloud.declared_latency_ms = 1;
    router
        .register_implementation(&monitor, &token, cloud)
        .unwrap();
    router
        .set_rollout_stage(&monitor, &token, ImplId(1), RolloutStage::Ga)
        .unwrap();

    let local = native_descriptor(2, "summarize", 3_000, 0.3, CostModel::Free);
    router
        .register_implementation(&monitor, &token, local)
        .unwrap();
    router
        .set_rollout_stage(&monitor, &token, ImplId(2), RolloutStage::Ga)
        .unwrap();

    let decision = router.route(&invocation(
        "summarize",
        UrgencyClass::Interactive,
        ConsequenceTier::Routine,
        false,
    ));

    assert_eq!(
        decision.chosen,
        Some(ImplId(2)),
        "cloud candidate must never be chosen without consent"
    );
    assert!(decision
        .rationale
        .candidates_excluded
        .iter()
        .any(|(id, _)| *id == ImplId(1)));
}

#[test]
fn cloud_candidate_becomes_eligible_only_with_explicit_consent() {
    let (monitor, token) = monitor_and_token();
    let router = router();
    router
        .register_implementation(&monitor, &token, cloud_descriptor(1, "summarize", 0.9))
        .unwrap();
    router
        .set_rollout_stage(&monitor, &token, ImplId(1), RolloutStage::Ga)
        .unwrap();

    let no_consent = router.route(&invocation(
        "summarize",
        UrgencyClass::Batch,
        ConsequenceTier::Routine,
        false,
    ));
    assert_eq!(no_consent.chosen, None);

    let with_consent = router.route(&invocation(
        "summarize",
        UrgencyClass::Batch,
        ConsequenceTier::Routine,
        true,
    ));
    assert_eq!(with_consent.chosen, Some(ImplId(1)));
}

#[test]
fn shadow_stage_candidates_are_never_chosen() {
    let (monitor, token) = monitor_and_token();
    let router = router();
    router
        .register_implementation(
            &monitor,
            &token,
            native_descriptor(1, "translate", 100, 0.99, CostModel::Free),
        )
        .unwrap();
    // Never promoted past Shadow (registration always enters at Shadow).

    let decision = router.route(&invocation(
        "translate",
        UrgencyClass::Interactive,
        ConsequenceTier::Routine,
        false,
    ));
    assert_eq!(decision.chosen, None);
    assert!(decision.rationale.candidates_considered.is_empty());
}

#[test]
fn interactive_urgency_favors_the_faster_candidate_over_the_higher_quality_one() {
    let (monitor, token) = monitor_and_token();
    let router = router();
    let fast_low_quality = native_descriptor(1, "chat", 50, 0.5, CostModel::Free);
    // Exceeds the invocation's 5s latency_budget_ms, so its latency_fit
    // actually decays below 1.0 — otherwise both candidates score a flat
    // 1.0 on latency and the test wouldn't be exercising the trade-off it
    // claims to.
    let slow_high_quality = native_descriptor(2, "chat", 8_000, 0.95, CostModel::Free);
    router
        .register_implementation(&monitor, &token, fast_low_quality)
        .unwrap();
    router
        .set_rollout_stage(&monitor, &token, ImplId(1), RolloutStage::Ga)
        .unwrap();
    router
        .register_implementation(&monitor, &token, slow_high_quality)
        .unwrap();
    router
        .set_rollout_stage(&monitor, &token, ImplId(2), RolloutStage::Ga)
        .unwrap();

    let decision = router.route(&invocation(
        "chat",
        UrgencyClass::Interactive,
        ConsequenceTier::Routine,
        false,
    ));
    assert_eq!(
        decision.chosen,
        Some(ImplId(1)),
        "Interactive weights latency heavily enough to prefer the fast candidate"
    );
}

#[test]
fn high_stakes_floors_quality_weight_and_picks_the_higher_quality_candidate() {
    let (monitor, token) = monitor_and_token();
    let router = router();
    let fast_low_quality = native_descriptor(1, "contract_review", 50, 0.5, CostModel::Free);
    let slow_high_quality = native_descriptor(2, "contract_review", 4_500, 0.95, CostModel::Free);
    router
        .register_implementation(&monitor, &token, fast_low_quality)
        .unwrap();
    router
        .set_rollout_stage(&monitor, &token, ImplId(1), RolloutStage::Ga)
        .unwrap();
    router
        .register_implementation(&monitor, &token, slow_high_quality)
        .unwrap();
    router
        .set_rollout_stage(&monitor, &token, ImplId(2), RolloutStage::Ga)
        .unwrap();

    let decision = router.route(&invocation(
        "contract_review",
        UrgencyClass::Interactive,
        ConsequenceTier::HighStakes,
        false,
    ));
    assert_eq!(
        decision.chosen,
        Some(ImplId(2)),
        "HighStakes must floor quality weight over Interactive's latency preference"
    );
    assert!(
        decision.rationale.needs_verification,
        "HighStakes must always flag ensemble verification as needed"
    );
}

#[test]
fn circuit_breaker_demotes_but_does_not_remove_a_failing_candidate() {
    let (monitor, token) = monitor_and_token();
    let router = router();
    let flaky = native_descriptor(1, "search", 100, 0.9, CostModel::Free);
    let reliable = native_descriptor(2, "search", 100, 0.5, CostModel::Free);
    router
        .register_implementation(&monitor, &token, flaky)
        .unwrap();
    router
        .set_rollout_stage(&monitor, &token, ImplId(1), RolloutStage::Ga)
        .unwrap();
    router
        .register_implementation(&monitor, &token, reliable)
        .unwrap();
    router
        .set_rollout_stage(&monitor, &token, ImplId(2), RolloutStage::Ga)
        .unwrap();

    let before = router.route(&invocation(
        "search",
        UrgencyClass::Background,
        ConsequenceTier::Routine,
        false,
    ));
    assert_eq!(
        before.chosen,
        Some(ImplId(1)),
        "higher quality wins before any failures"
    );

    for _ in 0..3 {
        router.report_outcome(ImplId(1), false);
    }

    let after = router.route(&invocation(
        "search",
        UrgencyClass::Background,
        ConsequenceTier::Routine,
        false,
    ));
    assert_eq!(
        after.chosen,
        Some(ImplId(2)),
        "3 consecutive failures must demote the flaky candidate below the reliable one"
    );
    assert!(
        after.fallback_chain.contains(&ImplId(1)),
        "the circuit breaker demotes, it never removes — docs/23 §Recovery Mechanisms"
    );

    router.report_outcome(ImplId(1), true);
    let recovered = router.route(&invocation(
        "search",
        UrgencyClass::Background,
        ConsequenceTier::Routine,
        false,
    ));
    assert_eq!(
        recovered.chosen,
        Some(ImplId(1)),
        "a successful outcome resets the breaker"
    );
}

#[test]
fn no_candidates_at_all_degrades_gracefully_instead_of_panicking() {
    let router = router();
    let decision = router.route(&invocation(
        "nonexistent",
        UrgencyClass::Interactive,
        ConsequenceTier::Routine,
        false,
    ));
    assert_eq!(decision.chosen, None);
    assert!(decision.fallback_chain.is_empty());
}

#[test]
fn registering_or_promoting_a_candidate_requires_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();
    let router = router();

    let result =
        router.register_implementation(&monitor, &read_only, cloud_descriptor(1, "x", 0.5));
    assert!(matches!(
        result,
        Err(hyperion_model_router::ModelRouterError::Unauthorized)
    ));

    router
        .register_implementation(&monitor, &root, cloud_descriptor(1, "x", 0.5))
        .unwrap();
    let result = router.set_rollout_stage(&monitor, &read_only, ImplId(1), RolloutStage::Ga);
    assert!(matches!(
        result,
        Err(hyperion_model_router::ModelRouterError::Unauthorized)
    ));
}
