use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_observability::{AuditAction, AuditLedger, AuditPayload, PrincipalRef};

use crate::types::{DegradationPlan, ScalabilityError};

fn require(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    rights: RightsMask,
) -> Result<(), ScalabilityError> {
    monitor
        .check_rights_ok_result(token, rights)
        .map_err(|_| ScalabilityError::Unauthorized)
}

/// docs/37 §3's `apply_and_explain`: "writes the audit notice atomically
/// with installing the substitution" — closing the doc's own named
/// "explanation lag" failure mode, where a degraded Capability runs
/// silently before its notice catches up. This crate does not itself
/// install the substituted implementation (see this crate's doc
/// comment on the deferred `hyperion-plugin-framework` wiring); it
/// writes the real, tamper-evident `hyperion-observability` audit entry
/// that must exist before or alongside that install.
pub fn apply_and_explain(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    audit: &AuditLedger,
    actor: PrincipalRef,
    plan: &DegradationPlan,
    now: u64,
) -> Result<(), ScalabilityError> {
    require(monitor, token, RightsMask::WRITE)?;
    audit.append(
        monitor,
        token,
        actor,
        AuditAction::AdminOverride,
        Some(plan.capability_ref.clone()),
        AuditPayload::Note(plan.notice.clone()),
        now,
    )?;
    Ok(())
}
