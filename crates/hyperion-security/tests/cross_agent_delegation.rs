//! docs/17 T3: a receiving Agent must never honor a sender's claimed risk
//! level — only its own independent re-assessment.

use hyperion_security::{
    assess, cross_agent_delegation_verify, InterventionLevel, PendingAction, SensitivityHint,
};

fn trivial_action(action_id: u64) -> PendingAction {
    PendingAction {
        action_id,
        object_refs: Vec::new(),
        scope_size: 1,
        reversible: true,
        sensitivity: SensitivityHint::Public,
        intent_confidence: 1.0,
        corroboration: 1.0,
        provenance: None,
    }
}

#[test]
fn a_senders_optimistic_claim_does_not_downgrade_the_receivers_own_risky_action() {
    let sender_claim = assess(&trivial_action(1)); // SilentProceed
    assert_eq!(
        sender_claim.intervention_level,
        InterventionLevel::SilentProceed
    );

    let receiver_action = PendingAction {
        scope_size: 100,
        reversible: false,
        sensitivity: SensitivityHint::Restricted,
        ..trivial_action(2)
    };

    let receiver_assessment = cross_agent_delegation_verify(&sender_claim, &receiver_action);
    assert_eq!(
        receiver_assessment.intervention_level,
        InterventionLevel::RequireBackupFirst,
        "the receiver's own independent assessment must win, never the sender's laundered claim"
    );
}

#[test]
fn a_senders_pessimistic_claim_does_not_upgrade_a_genuinely_trivial_receiver_action() {
    let sender_claim = assess(&PendingAction {
        scope_size: 100,
        reversible: false,
        sensitivity: SensitivityHint::Restricted,
        ..trivial_action(1)
    });
    assert_eq!(
        sender_claim.intervention_level,
        InterventionLevel::RequireBackupFirst
    );

    let receiver_assessment = cross_agent_delegation_verify(&sender_claim, &trivial_action(2));
    assert_eq!(
        receiver_assessment.intervention_level,
        InterventionLevel::SilentProceed
    );
}
