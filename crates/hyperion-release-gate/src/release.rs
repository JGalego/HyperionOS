use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_observability::{AuditAction, AuditLedger, AuditLogEntry, AuditPayload, PrincipalRef};

use crate::types::{
    GateOutcome, HardwareReleaseCriteria, ReleaseDecision, ReleaseGateError, ReleaseGateReport,
    SuiteReport,
};

fn require(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    rights: RightsMask,
) -> Result<(), ReleaseGateError> {
    monitor
        .check_rights_ok_result(token, rights)
        .map_err(|_| ReleaseGateError::Unauthorized)
}

/// docs/35 §1's `ReleaseGate.evaluate(build) -> ReleaseDecision`: a build
/// passes only if every sub-suite is non-blocking (per
/// [`SuiteReport::is_blocking`]), the benchmark regression check
/// (docs/36) did not return [`GateOutcome::Blocked`], and (docs/998-roadmap.md M13)
/// `hardware`'s own criteria are met — the one consolidated gate/report spanning both docs'
/// suites plus this roadmap's own real hardware/boot surface, matching each doc's own "Release
/// candidate" convergence point in its architecture diagram.
pub fn evaluate_release(
    build_id: &str,
    suites: &[SuiteReport],
    benchmark_outcome: Option<GateOutcome>,
    hardware: &HardwareReleaseCriteria,
) -> ReleaseGateReport {
    let blocking_suites: Vec<_> = suites
        .iter()
        .filter(|s| s.is_blocking())
        .map(|s| s.kind)
        .collect();
    let benchmark_blocks = benchmark_outcome == Some(GateOutcome::Blocked);
    let hardware_criteria_met = hardware.is_met();

    let decision = if blocking_suites.is_empty() && !benchmark_blocks && hardware_criteria_met {
        ReleaseDecision::Pass
    } else {
        ReleaseDecision::Blocked
    };
    ReleaseGateReport {
        build_id: build_id.to_string(),
        decision,
        blocking_suites,
        benchmark_outcome,
        hardware_criteria_met,
    }
}

/// docs/35 §1's completeness invariant: "every release-gate decision
/// must correspond to exactly one signed entry in \[34\]'s audit
/// ledger." This is that write — every call to
/// [`evaluate_release`] a caller intends to act on should be followed by
/// exactly one call here.
pub fn record_release_decision(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    audit: &AuditLedger,
    report: &ReleaseGateReport,
    now: u64,
) -> Result<(), ReleaseGateError> {
    require(monitor, token, RightsMask::WRITE)?;
    audit.append(
        monitor,
        token,
        PrincipalRef::System,
        AuditAction::AdminOverride,
        Some(report.build_id.clone()),
        AuditPayload::Note(format!(
            "release decision: {:?} (blocking: {:?})",
            report.decision, report.blocking_suites
        )),
        now,
    )?;
    Ok(())
}

/// docs/35 §1's "completeness verifier over the ledger and release
/// history": every build id in `expected_build_ids` must correspond to
/// at least one recorded decision; anything missing is returned as a
/// gap — a release that shipped with no corresponding audit entry,
/// which the doc treats as the specific failure this verifier exists to
/// catch.
pub fn verify_completeness(
    audit_entries: &[AuditLogEntry],
    expected_build_ids: &[String],
) -> Vec<String> {
    expected_build_ids
        .iter()
        .filter(|id| {
            !audit_entries
                .iter()
                .any(|e| e.target.as_deref() == Some(id.as_str()))
        })
        .cloned()
        .collect()
}
