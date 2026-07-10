//! docs/23-multi-model-orchestration.md's routing pipeline: the privacy
//! gate as a hard exclusion never rescuable by scoring, shadow-stage
//! exclusion from the returned decision, urgency/consequence-tier-driven
//! weight shifts, and the circuit breaker.

use std::collections::HashMap;
use std::sync::Arc;

use hyperion_ai_runtime::{LocalAiRuntime, MockBackend};
use hyperion_model_router::{
    CapabilityInvocation, ConsequenceTier, CostModel, ImplId, ImplKind, ImplementationDescriptor,
    ModelRouter, PrivacyTier, RolloutStage, UrgencyClass,
};

fn router() -> ModelRouter {
    ModelRouter::new(Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000)))
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
    let router = router();
    // A cloud candidate engineered to win on every other axis: perfect
    // quality, free, fast.
    let mut cloud = cloud_descriptor(1, "summarize", 1.0);
    cloud.cost_model = CostModel::Free;
    cloud.declared_latency_ms = 1;
    router.register_implementation(cloud);
    router.set_rollout_stage(ImplId(1), RolloutStage::Ga);

    let local = native_descriptor(2, "summarize", 3_000, 0.3, CostModel::Free);
    router.register_implementation(local);
    router.set_rollout_stage(ImplId(2), RolloutStage::Ga);

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
    let router = router();
    router.register_implementation(cloud_descriptor(1, "summarize", 0.9));
    router.set_rollout_stage(ImplId(1), RolloutStage::Ga);

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
    let router = router();
    router.register_implementation(native_descriptor(
        1,
        "translate",
        100,
        0.99,
        CostModel::Free,
    ));
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
    let router = router();
    let fast_low_quality = native_descriptor(1, "chat", 50, 0.5, CostModel::Free);
    // Exceeds the invocation's 5s latency_budget_ms, so its latency_fit
    // actually decays below 1.0 — otherwise both candidates score a flat
    // 1.0 on latency and the test wouldn't be exercising the trade-off it
    // claims to.
    let slow_high_quality = native_descriptor(2, "chat", 8_000, 0.95, CostModel::Free);
    router.register_implementation(fast_low_quality);
    router.set_rollout_stage(ImplId(1), RolloutStage::Ga);
    router.register_implementation(slow_high_quality);
    router.set_rollout_stage(ImplId(2), RolloutStage::Ga);

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
    let router = router();
    let fast_low_quality = native_descriptor(1, "contract_review", 50, 0.5, CostModel::Free);
    let slow_high_quality = native_descriptor(2, "contract_review", 4_500, 0.95, CostModel::Free);
    router.register_implementation(fast_low_quality);
    router.set_rollout_stage(ImplId(1), RolloutStage::Ga);
    router.register_implementation(slow_high_quality);
    router.set_rollout_stage(ImplId(2), RolloutStage::Ga);

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
    let router = router();
    let flaky = native_descriptor(1, "search", 100, 0.9, CostModel::Free);
    let reliable = native_descriptor(2, "search", 100, 0.5, CostModel::Free);
    router.register_implementation(flaky);
    router.set_rollout_stage(ImplId(1), RolloutStage::Ga);
    router.register_implementation(reliable);
    router.set_rollout_stage(ImplId(2), RolloutStage::Ga);

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
