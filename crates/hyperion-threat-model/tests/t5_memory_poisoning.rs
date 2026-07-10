//! docs/17 T5: memory poisoning — a maximally poisoned corroboration
//! signal must never buy down the irreversible-and-wide-blast-radius
//! floor. See `hyperion-security`'s own test suite for the full
//! composite-score regression; this is the threat-registry-facing
//! restatement of the same invariant.

use hyperion_security::{assess, InterventionLevel, PendingAction, SensitivityHint};

#[test]
fn t5_a_maximally_poisoned_corroboration_signal_cannot_downgrade_an_irreversible_wide_blast_action()
{
    let action = PendingAction {
        action_id: 1,
        object_refs: vec![],
        scope_size: 1000,
        reversible: false,
        sensitivity: SensitivityHint::Restricted,
        intent_confidence: 1.0,
        corroboration: 1.0,
        provenance: None,
    };

    let assessment = assess(&action);
    assert!(
        assessment.composite_score < 0.75,
        "the weighted score alone would only reach require-explicit-confirm"
    );
    assert_eq!(assessment.intervention_level, InterventionLevel::RequireBackupFirst, "the unconditional floor must still fire despite maximal (attacker-controlled) corroboration");
}
