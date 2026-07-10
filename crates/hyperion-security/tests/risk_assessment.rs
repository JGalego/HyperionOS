//! docs/15 §7's risk-assessment algorithm and its two unconditional
//! floors — the Phase 8 exit criterion ("deleting many Semantic Objects
//! correctly triggers backup-then-confirm") and docs/17 T5's exact
//! concern (a poisoned corroboration signal must never buy down the
//! irreversible-and-wide-blast-radius floor).

use hyperion_security::{assess, InterventionLevel, PendingAction, SensitivityHint};

fn base_action() -> PendingAction {
    PendingAction {
        action_id: 1,
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
fn a_small_reversible_low_sensitivity_action_proceeds_silently() {
    let action = base_action();
    let assessment = assess(&action);
    assert_eq!(
        assessment.intervention_level,
        InterventionLevel::SilentProceed
    );
}

#[test]
fn deleting_many_semantic_objects_irreversibly_triggers_backup_first() {
    let action = PendingAction {
        scope_size: 50,
        reversible: false,
        sensitivity: SensitivityHint::Personal,
        intent_confidence: 0.9,
        corroboration: 0.5,
        ..base_action()
    };
    let assessment = assess(&action);
    assert_eq!(
        assessment.intervention_level,
        InterventionLevel::RequireBackupFirst
    );
}

#[test]
fn a_poisoned_corroboration_signal_cannot_buy_down_the_backup_first_floor() {
    // Maximal blast radius, fully irreversible, fully sensitive, zero
    // confidence — but an attacker has poisoned the corroboration signal
    // to its maximum (1.0), which the weighted formula alone would allow
    // to pull the composite score down. The floor must still fire.
    let action = PendingAction {
        scope_size: 1000,
        reversible: false,
        sensitivity: SensitivityHint::Restricted,
        intent_confidence: 1.0,
        corroboration: 1.0,
        ..base_action()
    };
    let assessment = assess(&action);
    // Without the floor, the weighted composite alone (0.65) would only
    // reach `RequireExplicitConfirm` — the floor is what forces backup-first.
    assert!(assessment.composite_score < 0.75);
    assert_eq!(
        assessment.intervention_level,
        InterventionLevel::RequireBackupFirst,
        "poisoned corroboration must never buy down the irreversible+wide-blast-radius floor"
    );
}

#[test]
fn tainted_provenance_floors_at_require_explicit_confirm_even_for_an_otherwise_trivial_action() {
    use hyperion_security::{IntentProvenanceChain, OriginType, ProvenanceNode};

    let action = PendingAction {
        scope_size: 1,
        reversible: true,
        sensitivity: SensitivityHint::Public,
        intent_confidence: 1.0,
        corroboration: 1.0,
        provenance: Some(IntentProvenanceChain {
            action_id: 1,
            originating_intent_id: 7,
            derivation_path: vec![ProvenanceNode {
                origin_type: OriginType::IngestedExternal,
                user_confirmed: false,
            }],
        }),
        ..base_action()
    };
    let assessment = assess(&action);
    assert_eq!(
        assessment.intervention_level,
        InterventionLevel::RequireExplicitConfirm
    );
}

#[test]
fn a_confirmed_ingested_source_is_not_tainted() {
    use hyperion_security::{IntentProvenanceChain, OriginType, ProvenanceNode};

    let action = PendingAction {
        provenance: Some(IntentProvenanceChain {
            action_id: 1,
            originating_intent_id: 7,
            derivation_path: vec![ProvenanceNode {
                origin_type: OriginType::IngestedExternal,
                user_confirmed: true,
            }],
        }),
        ..base_action()
    };
    let assessment = assess(&action);
    assert_eq!(
        assessment.intervention_level,
        InterventionLevel::SilentProceed
    );
}

#[test]
fn moderate_risk_requires_notification_not_silence_or_a_hard_block() {
    let action = PendingAction {
        scope_size: 5,
        reversible: true,
        sensitivity: SensitivityHint::Sensitive,
        intent_confidence: 0.7,
        corroboration: 0.3,
        ..base_action()
    };
    let assessment = assess(&action);
    assert_eq!(
        assessment.intervention_level,
        InterventionLevel::NotifyAndProceed
    );
}
