use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_recovery::{RecoveryService, Trigger};

use crate::types::{InterventionLevel, PendingAction, RiskAssessment, SecurityError};

/// docs/15 §7's four score bands.
const SILENT_MAX: f32 = 0.2;
const NOTIFY_MAX: f32 = 0.45;
const CONFIRM_MAX: f32 = 0.75;

/// docs/15 §7: how many distinct touched objects saturate the blast-
/// radius score to `1.0` — a hosted-simulator-appropriate constant, not a
/// value docs/15 itself pins down.
const BLAST_RADIUS_SATURATION: f32 = 10.0;

fn level_for_score(score: f32) -> InterventionLevel {
    if score < SILENT_MAX {
        InterventionLevel::SilentProceed
    } else if score < NOTIFY_MAX {
        InterventionLevel::NotifyAndProceed
    } else if score < CONFIRM_MAX {
        InterventionLevel::RequireExplicitConfirm
    } else {
        InterventionLevel::RequireBackupFirst
    }
}

fn score_blast_radius(action: &PendingAction) -> f32 {
    (action.scope_size as f32 / BLAST_RADIUS_SATURATION).min(1.0)
}

fn score_reversibility(action: &PendingAction) -> f32 {
    if action.reversible {
        1.0
    } else {
        0.0
    }
}

/// docs/15 §7's risk-assessment algorithm, exactly: a weighted composite
/// plus two unconditional floors that override it — the floors are
/// literal checks, not folded into the weighted sum, because the -0.10
/// corroboration weight could otherwise pull a maximal-risk action just
/// under the backup-first threshold (docs/17 T5's exact concern: a
/// poisoned "corroborating" memory must never buy down the floor).
pub fn assess(action: &PendingAction) -> RiskAssessment {
    let blast = score_blast_radius(action);
    let revers = score_reversibility(action);
    let sensit = action.sensitivity.score();
    let conf = action.intent_confidence.clamp(0.0, 1.0);
    let corrob = action.corroboration.clamp(0.0, 1.0);

    let composite = (0.30 * blast + 0.25 * (1.0 - revers) + 0.20 * sensit + 0.15 * (1.0 - conf)
        - 0.10 * corrob)
        .clamp(0.0, 1.0);

    let mut floor = InterventionLevel::SilentProceed;
    let tainted = action.provenance.as_ref().is_some_and(|c| c.is_tainted());
    if tainted {
        floor = InterventionLevel::RequireExplicitConfirm;
    }
    let irreversible_and_wide = revers <= 0.05 && blast >= 0.8;
    if irreversible_and_wide {
        floor = floor.max(InterventionLevel::RequireBackupFirst);
    }

    let level = level_for_score(composite).max(floor);

    let rationale = format!(
        "composite={composite:.2} (blast={blast:.2}, reversibility={revers:.2}, sensitivity={sensit:.2}, confidence={conf:.2}, corroboration={corrob:.2}){}{}",
        if tainted { "; tainted provenance floors at require-explicit-confirm" } else { "" },
        if irreversible_and_wide { "; irreversible+wide-blast-radius floors at require-backup-first" } else { "" },
    );

    RiskAssessment {
        action_id: action.action_id,
        blast_radius_score: blast,
        reversibility_score: revers,
        sensitivity_score: sensit,
        confidence_score: conf,
        corroboration_score: corrob,
        composite_score: composite,
        intervention_level: level,
        rationale,
        recovery_point_ref: None,
    }
}

fn require(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    rights: RightsMask,
) -> Result<(), SecurityError> {
    monitor
        .check_rights_ok_result(token, rights)
        .map_err(|_| SecurityError::Unauthorized)
}

/// docs/15 §7 + docs/33's relationship: "the risk-assessment engine calls
/// `recovery_point_create` synchronously, in the request path, before any
/// action classified `RequireBackupFirst` is allowed to execute — the
/// recovery point is a precondition of execution, not an afterthought."
pub fn assess_and_prepare(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    recovery: &RecoveryService,
    action: &PendingAction,
    now: u64,
) -> Result<RiskAssessment, SecurityError> {
    require(monitor, token, RightsMask::EXEC)?;

    let mut assessment = assess(action);
    if assessment.intervention_level == InterventionLevel::RequireBackupFirst {
        let point = recovery.recovery_point_create(
            monitor,
            token,
            Trigger::PreRiskyAction,
            &action.object_refs,
            now,
        )?;
        assessment.recovery_point_ref = Some(point);
    }
    Ok(assessment)
}

/// docs/17 T3's "no delegated risk assessment" rule: a receiving Agent
/// must never honor a sender's claimed risk level, only its own
/// independent re-assessment against the receiver's own inputs. This
/// function's entire value is that invariant, not its (trivial)
/// implementation — see this crate's tests for the adversarial case a
/// sender claiming `SilentProceed` does not get honored.
pub fn cross_agent_delegation_verify(
    _sender_claimed: &RiskAssessment,
    receiver_action: &PendingAction,
) -> RiskAssessment {
    assess(receiver_action)
}
