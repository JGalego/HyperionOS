use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_observability::{AuditAction, AuditLedger, AuditPayload, PrincipalRef};
use hyperion_plugin_framework::PluginRegistry;

use crate::types::{DegradationOutcome, DegradationPlan, ScalabilityError, Substitution};

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
/// install a substituted implementation through
/// `hyperion-plugin-framework` (a full `CapabilityManifest` isn't
/// something a bare [`Substitution`] carries enough information to
/// construct — see this crate's own doc comment); when `registry` is
/// supplied, though, an `AlternateImplementation` substitution is
/// confirmed against it — a valid target must already be a real,
/// registered, non-quarantined capability, since nothing else could have
/// produced a real fallback — before the audit notice is written, so this
/// never claims a substitution happened against a capability that isn't
/// actually there to run. `registry: None` (no real registry to check
/// against) skips this check entirely, matching every other caller-
/// optional integration in this workspace.
pub fn apply_and_explain(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    audit: &AuditLedger,
    actor: PrincipalRef,
    plan: &DegradationPlan,
    registry: Option<&PluginRegistry>,
    now: u64,
) -> Result<(), ScalabilityError> {
    require(monitor, token, RightsMask::WRITE)?;

    if let Some(registry) = registry {
        if let DegradationOutcome::Substituted {
            substitution: Substitution::AlternateImplementation(capability_ref),
        } = &plan.outcome
        {
            if registry.query(capability_ref).is_none() {
                return Err(ScalabilityError::AlternateImplementationNotRegistered(
                    capability_ref.clone(),
                ));
            }
        }
    }

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
