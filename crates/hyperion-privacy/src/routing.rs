use crate::consent::ConsentLedger;
use crate::types::{
    DataScope, DegradeReason, PrivacyProfile, PrivacyTier, ResidencyTag, RoutingDecision,
};

/// docs/16 §5/§7's `route_capability_call` — the deny-by-default gate
/// docs/16 says [23 — Multi-Model Orchestration](../23-multi-model-orchestration.md)'s
/// own `privacy_gate` calls into rather than duplicates: "one routing
/// decision, made by 23, informed at the privacy step by this logic." A
/// refused candidate is removed from 23's candidate set before scoring,
/// not merely deprioritized. The hard invariant this function embodies:
/// no path may reach `DispatchCloud` without a live [`crate::ConsentGrant`]
/// on the call stack — absence of a grant, whether never-issued or
/// revoked, always degrades, never defaults to allow.
#[allow(clippy::too_many_arguments)]
pub fn route_capability_call(
    profile: &PrivacyProfile,
    domain: &str,
    scope: &DataScope,
    residency: Option<&ResidencyTag>,
    has_local_impl: bool,
    ledger: &ConsentLedger,
    subject: u64,
    now: u64,
) -> RoutingDecision {
    let tier = profile.tier_for(domain);

    match tier {
        PrivacyTier::FullyLocal => {
            if has_local_impl {
                RoutingDecision::DispatchLocal
            } else {
                RoutingDecision::Degraded(DegradeReason::NoLocalImplementation)
            }
        }
        PrivacyTier::LocalPreferredWithConsent => {
            if has_local_impl {
                return RoutingDecision::DispatchLocal;
            }
            if residency.is_some_and(|tag| tag.forbids(PrivacyTier::CloudAssisted)) {
                return RoutingDecision::Degraded(DegradeReason::ResidencyForbidsCloud);
            }
            match ledger.standing_grant(subject, scope, now) {
                Some(grant) => RoutingDecision::DispatchCloud { grant_id: grant.id },
                None => RoutingDecision::Degraded(DegradeReason::NoStandingConsent),
            }
        }
        PrivacyTier::CloudAssisted => {
            if residency.is_some_and(|tag| tag.forbids(PrivacyTier::CloudAssisted)) {
                return RoutingDecision::Degraded(DegradeReason::ResidencyForbidsCloud);
            }
            match ledger.standing_grant(subject, scope, now) {
                Some(grant) => RoutingDecision::DispatchCloud { grant_id: grant.id },
                None if has_local_impl => RoutingDecision::DispatchLocal,
                None => RoutingDecision::Degraded(DegradeReason::NoStandingConsent),
            }
        }
    }
}

/// docs/16 §5's least-privilege context assembly ("build for recipient
/// contract"): an object is included only if the recipient's declared
/// sub-intent actually needs it *and* the object's residency permits the
/// tier the recipient runs under — exclusion is reported with a reason
/// (feeding [18 — Explainability & Trust](../18-explainability-and-trust.md)),
/// never silent.
pub fn filter_for_recipient<'a>(
    objects: impl IntoIterator<Item = &'a ResidencyTag>,
    recipient_declared_need: &std::collections::HashSet<hyperion_knowledge_graph::NodeId>,
    recipient_tier: PrivacyTier,
) -> (
    Vec<hyperion_knowledge_graph::NodeId>,
    Vec<(hyperion_knowledge_graph::NodeId, String)>,
) {
    let mut included = Vec::new();
    let mut withheld = Vec::new();

    for tag in objects {
        if !recipient_declared_need.contains(&tag.object_id) {
            withheld.push((
                tag.object_id,
                "not declared needed by recipient's sub-intent".to_string(),
            ));
            continue;
        }
        if tag.forbids(recipient_tier) {
            withheld.push((
                tag.object_id,
                format!("object's residency forbids tier {recipient_tier:?}"),
            ));
            continue;
        }
        included.push(tag.object_id);
    }

    (included, withheld)
}
