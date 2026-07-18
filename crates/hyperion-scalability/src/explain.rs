use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_observability::{AuditAction, AuditLedger, AuditPayload, PrincipalRef};
use hyperion_plugin_framework::{PluginRegistry, RegistryEntry};

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
/// call `hyperion-plugin-framework`'s own `PluginRegistry::install` (a
/// full, signed `PluginManifest` is genuinely more than a bare
/// [`Substitution`] carries enough information to construct, and
/// installation is that crate's own capability-gated, signature-verified
/// operation — not something to fabricate a manifest to route around);
/// when `registry` is supplied, an `AlternateImplementation` substitution
/// is instead confirmed against it, and the real, live
/// [`RegistryEntry`] that confirmation reads is now genuinely returned —
/// docs/37's own pseudocode names this `installed` and hands it back to
/// the caller, and previously this function discarded exactly that value
/// after using it only to validate. `Ok(None)` covers every case with no
/// real `PluginRegistry` concept to return: `registry: None` (no real
/// registry to check against, matching every other caller-optional
/// integration in this workspace), and every non-`AlternateImplementation`
/// outcome (`CheaperLocalTier`/`ConsentedCloudUpgrade` aren't
/// pre-registered capabilities the same way — see this crate's own doc
/// comment).
pub fn apply_and_explain(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    audit: &AuditLedger,
    actor: PrincipalRef,
    plan: &DegradationPlan,
    registry: Option<&PluginRegistry>,
    now: u64,
) -> Result<Option<RegistryEntry>, ScalabilityError> {
    require(monitor, token, RightsMask::WRITE)?;

    let mut installed = None;
    if let Some(registry) = registry {
        if let DegradationOutcome::Substituted {
            substitution: Substitution::AlternateImplementation(capability_ref, _),
        } = &plan.outcome
        {
            let entry = registry.query(capability_ref).ok_or_else(|| {
                ScalabilityError::AlternateImplementationNotRegistered(capability_ref.clone())
            })?;
            installed = Some(entry);
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
    Ok(installed)
}
