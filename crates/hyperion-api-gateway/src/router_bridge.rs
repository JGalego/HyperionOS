use hyperion_explainability::{Alternative, ConfidenceMethod, ConfidenceScore};
use hyperion_model_router::{
    CapabilityInvocation, ConsequenceTier, CostModel, ExclusionReason, ImplId,
    ImplKind as RouterImplKind, ImplementationDescriptor as RouterImplementationDescriptor,
    PrivacyTier as RouterPrivacyTier, RolloutStage, RoutingDecision, UrgencyClass,
};
use hyperion_plugin_framework::{
    ImplementationDescriptor as PluginImplementationDescriptor,
    ImplementationKind as PluginImplKind,
};
use hyperion_security::InterventionLevel;

/// Bridges `hyperion-plugin-framework`'s registry shape to
/// `hyperion-model-router`'s own — the adapter both crates' doc comments
/// name as the missing piece ("no adapter from `hyperion-plugin-
/// framework`'s registry shape to `hyperion-model-router`'s own shape
/// yet"). Lives here, in the gateway, rather than in either subsystem
/// crate: `hyperion-model-router` explicitly doesn't want a dependency on
/// the Plugin Framework (its own doc comment: "candidates are registered
/// directly... rather than discovered from a real plugin registry"), and
/// `hyperion-plugin-framework` has no reason to know Model Router's
/// scoring shape exists. The gateway already depends on both, so it is
/// the natural, decoupling seam.
fn to_router_kind(kind: PluginImplKind) -> RouterImplKind {
    match kind {
        PluginImplKind::LocalSmallModel => RouterImplKind::LocalSmallModel,
        PluginImplKind::LocalLargeModel => RouterImplKind::LocalLargeModel,
        PluginImplKind::CloudApi => RouterImplKind::CloudApi,
        PluginImplKind::NativeBinary => RouterImplKind::NativeBinary,
    }
}

/// `plugin_id` doubles as the router's `ImplId` — both are already plain
/// `u64` identities naming "one specific implementation," so no new
/// identity space is minted for the bridge.
pub(crate) fn to_router_descriptor(
    descriptor: &PluginImplementationDescriptor,
    capability_id: &str,
) -> RouterImplementationDescriptor {
    let mut quality_profile = std::collections::HashMap::new();
    quality_profile.insert(capability_id.to_string(), descriptor.quality_score);

    RouterImplementationDescriptor {
        impl_id: ImplId(descriptor.plugin_id),
        capability_id: capability_id.to_string(),
        kind: to_router_kind(descriptor.implementation_kind),
        // `hyperion-plugin-framework::ImplementationDescriptor` has no `ModelClass`-equivalent
        // field at all (just `plugin_id`/`implementation_kind`/`quality_score`/`version`), so
        // even a plugin manifest declaring `ImplementationKind::LocalSmallModel` bridges here
        // with no real model class to run — M8's real `ai_runtime.infer()` wiring in
        // `ApiGateway::dispatch_one` only ever fires for a candidate with `Some(class)`, so a
        // plugin-bridged local-model candidate still falls back to the stub dispatch today.
        // Giving the Plugin Framework's own manifest a real `ModelClass` field is a separate,
        // larger change to that crate's manifest/signature shape, not attempted here — the same
        // scoping this function's own `privacy_tier` gap below already documents.
        model_class: None,
        // docs/16's real per-implementation privacy tier isn't carried by
        // `hyperion-plugin-framework`'s manifest yet — every bridged
        // candidate is treated as `Local` until that's wired, a real gap
        // this crate's doc comment calls out rather than hides.
        privacy_tier: RouterPrivacyTier::Local,
        cost_model: CostModel::Free,
        quality_profile,
        declared_latency_ms: 200,
        rollout_stage: RolloutStage::Ga,
    }
}

/// Maps the real `hyperion-security` risk assessment already computed
/// for this same invocation onto the Model Router's `ConsequenceTier` —
/// a risky action gets real weight toward quality/privacy over latency/
/// cost in `WeightVector::for_invocation`, rather than every invocation
/// being scored as uniformly `Routine`. `SilentProceed`/`NotifyAndProceed`
/// both read as ordinary, unremarkable consequence; `RequireExplicitConfirm`
/// is `Sensitive`; `RequireBackupFirst` — the tier that already forced a
/// real recovery point — is `HighStakes`.
pub(crate) fn consequence_tier_for(level: InterventionLevel) -> ConsequenceTier {
    match level {
        InterventionLevel::SilentProceed | InterventionLevel::NotifyAndProceed => {
            ConsequenceTier::Routine
        }
        InterventionLevel::RequireExplicitConfirm => ConsequenceTier::Sensitive,
        InterventionLevel::RequireBackupFirst => ConsequenceTier::HighStakes,
    }
}

/// A `CapabilityInvocation` for a gateway-driven invocation.
/// `consequence_tier` is real (see [`consequence_tier_for`]);
/// `urgency_class`/`cloud_consent` remain permissive, deliberate
/// defaults — not oversights. `urgency_class` models *synchronous vs.
/// backgroundable execution*, not emotional urgency in phrasing:
/// `invoke_capability` is itself always a blocking call the caller waits
/// on, so `UrgencyClass::Interactive` is already the objectively correct
/// value here, not a placeholder standing in for a real derivation.
/// `cloud_consent` staying `true` is also deliberate, not deferred:
/// `hyperion-privacy::ConsentLedger` is this workspace's canonical
/// consent mechanism, but that crate's own doc comment explicitly asks
/// that `hyperion-model-router`'s already-shipped, already-tested
/// two-value privacy gate *not* be rewired onto it as a rushed
/// side-effect of unrelated work — "a real, separate migration," in its
/// own words.
pub(crate) fn build_invocation(
    capability_id: &str,
    consequence_tier: ConsequenceTier,
) -> CapabilityInvocation {
    CapabilityInvocation {
        capability_id: capability_id.to_string(),
        urgency_class: UrgencyClass::Interactive,
        consequence_tier,
        quality_floor: None,
        latency_budget_ms: 5_000,
        cloud_consent: true,
    }
}

/// Turns a real `hyperion-model-router` routing decision into
/// `hyperion-explainability`'s Confidence/Alternatives shape. The winning
/// candidate's own composite fitness score becomes the `ConfidenceScore`
/// — a real signal, not a fabricated placeholder: it's exactly how well
/// the router's weighted scoring fit this candidate, which is a
/// genuinely different thing from a `hyperion-security` risk score (see
/// this crate's doc comment on why that one is *not* reused here).
/// `ConfidenceMethod::Heuristic` is the correct tag since this is a
/// deterministic weighted-fit formula, not model self-consistency,
/// verification, or ensemble agreement. Every other candidate this
/// invocation considered (scored but not chosen) or excluded outright
/// (a hard gate rejected it before scoring) becomes an `Alternative`.
pub(crate) fn to_confidence_and_alternatives(
    decision: &RoutingDecision,
) -> (ConfidenceScore, Vec<Alternative>) {
    let winner_composite = decision
        .rationale
        .candidates_considered
        .first()
        .map(|(_, score)| score.composite)
        .unwrap_or(0.0);

    let confidence = ConfidenceScore {
        value: winner_composite,
        method: ConfidenceMethod::Heuristic,
    };

    let mut alternatives: Vec<Alternative> = decision
        .rationale
        .candidates_considered
        .iter()
        .skip(1)
        .map(|(impl_id, score)| Alternative {
            description: format!("implementation {}", impl_id.0),
            score: score.composite,
            rejection_reason: format!(
                "composite score {:.3} did not beat the winner's {:.3}",
                score.composite, winner_composite
            ),
        })
        .collect();

    alternatives.extend(
        decision
            .rationale
            .candidates_excluded
            .iter()
            .map(|(impl_id, reason)| Alternative {
                description: format!("implementation {}", impl_id.0),
                score: 0.0,
                rejection_reason: match reason {
                    ExclusionReason::PrivacyGate => {
                        "excluded by the privacy gate before scoring".to_string()
                    }
                    ExclusionReason::ResourceInfeasible => {
                        "excluded as not locally feasible before scoring".to_string()
                    }
                    ExclusionReason::CanaryNotSampled => {
                        "excluded: this call's traffic sample landed outside this canary's \
                         declared percentage"
                            .to_string()
                    }
                },
            }),
    );

    (confidence, alternatives)
}
