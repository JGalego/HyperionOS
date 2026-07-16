use hyperion_privacy::{ConsentLedger, DataScope};

use crate::types::{
    DegradationOutcome, DegradationPlan, DegradationPolicy, HardwareProfile, Substitution,
};

/// docs/37 §3's `degrade_capability` pseudocode, exactly: no policy, or a
/// policy whose constraint the hardware satisfies, is full fidelity;
/// otherwise walk `fallback_order` in its declared, fixed sequence.
/// `resolve_alternate_fits` stands in for `scheduler.would_fit(...)` —
/// pass [`crate::fit::scheduler_backed_resolver`] for a real
/// implementation backed by `hyperion_scheduler::Scheduler`'s live
/// ledgers (see that function's doc comment for exactly which
/// dimensions it checks and why). `ConsentedCloudUpgrade` is the one
/// branch with a real dependency here regardless of what's passed for
/// `resolve_alternate_fits`: it checks `hyperion-privacy`'s real
/// `ConsentLedger` for a standing grant scoped to the named provider,
/// never assuming consent, exactly like every other consent check in
/// this workspace.
pub fn degrade_capability(
    policy: Option<&DegradationPolicy>,
    profile: &HardwareProfile,
    consent_ledger: &ConsentLedger,
    subject: u64,
    resolve_alternate_fits: impl Fn(&Substitution) -> bool,
    now: u64,
) -> DegradationPlan {
    let Some(policy) = policy else {
        return DegradationPlan {
            capability_ref: String::new(),
            outcome: DegradationOutcome::FullFidelity,
            notice: "no degradation policy registered; running at full fidelity".to_string(),
        };
    };

    if !policy.constraint.violated_by(profile) {
        return DegradationPlan {
            capability_ref: policy.capability_ref.clone(),
            outcome: DegradationOutcome::FullFidelity,
            notice: format!(
                "'{}' runs at full fidelity on this device",
                policy.capability_ref
            ),
        };
    }

    for substitution in &policy.fallback_order {
        let accepted = match substitution {
            Substitution::CheaperLocalTier(_, _) | Substitution::AlternateImplementation(_, _) => {
                resolve_alternate_fits(substitution)
            }
            Substitution::ConsentedCloudUpgrade(provider) => consent_ledger
                .standing_grant(subject, &DataScope::Capability(provider.clone()), now)
                .is_some(),
            Substitution::Disable => false,
        };
        if accepted {
            return DegradationPlan {
                capability_ref: policy.capability_ref.clone(),
                outcome: DegradationOutcome::Substituted {
                    substitution: substitution.clone(),
                },
                notice: format!(
                    "'{}' substituted via {substitution:?} to fit this device",
                    policy.capability_ref
                ),
            };
        }
    }

    DegradationPlan {
        capability_ref: policy.capability_ref.clone(),
        outcome: DegradationOutcome::Disabled,
        notice: format!(
            "'{}' disabled: no fitting implementation on this device",
            policy.capability_ref
        ),
    }
}
