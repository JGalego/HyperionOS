//! docs/35 §1's `ReleaseGate.evaluate`: every suite must be non-blocking
//! and the benchmark gate must not be `Blocked`; the threat-regression
//! suite's provenance distinction is the one suite-specific exception to
//! "any failure blocks."

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_observability::AuditLedger;
use hyperion_release_gate::{
    evaluate_release, record_release_decision, verify_completeness, GateOutcome,
    HardwareReleaseCriteria, ReleaseDecision, ReleaseGateError, SuiteKind, SuiteReport,
};

fn clean_suite(kind: SuiteKind) -> SuiteReport {
    SuiteReport {
        kind,
        passed: 10,
        failed: 0,
        quarantined: 0,
        regressed_previously_mitigated: vec![],
    }
}

#[test]
fn a_build_with_every_suite_clean_and_no_benchmark_breach_passes() {
    let suites = vec![
        clean_suite(SuiteKind::Deterministic),
        clean_suite(SuiteKind::GoldenIntent),
        clean_suite(SuiteKind::ThreatRegression),
    ];
    let report = evaluate_release(
        "build-1",
        &suites,
        Some(GateOutcome::Pass),
        &HardwareReleaseCriteria::all_clear(),
    );
    assert_eq!(report.decision, ReleaseDecision::Pass);
    assert!(report.blocking_suites.is_empty());
}

#[test]
fn a_deterministic_suite_failure_blocks_release() {
    let mut deterministic = clean_suite(SuiteKind::Deterministic);
    deterministic.failed = 1;
    let report = evaluate_release(
        "build-1",
        &[deterministic],
        Some(GateOutcome::Pass),
        &HardwareReleaseCriteria::all_clear(),
    );
    assert_eq!(report.decision, ReleaseDecision::Blocked);
    assert_eq!(report.blocking_suites, vec![SuiteKind::Deterministic]);
}

#[test]
fn a_never_tested_threat_gap_does_not_block_release() {
    let mut threat = clean_suite(SuiteKind::ThreatRegression);
    threat.failed = 1; // a failure exists, but it's not in regressed_previously_mitigated
    let report = evaluate_release(
        "build-1",
        &[threat],
        Some(GateOutcome::Pass),
        &HardwareReleaseCriteria::all_clear(),
    );
    assert_eq!(
        report.decision,
        ReleaseDecision::Pass,
        "a never-catalogued gap must be tracked, not blocking"
    );
}

#[test]
fn a_previously_mitigated_threat_regressing_blocks_release() {
    let mut threat = clean_suite(SuiteKind::ThreatRegression);
    threat.regressed_previously_mitigated = vec!["T5".to_string()];
    let report = evaluate_release(
        "build-1",
        &[threat],
        Some(GateOutcome::Pass),
        &HardwareReleaseCriteria::all_clear(),
    );
    assert_eq!(report.decision, ReleaseDecision::Blocked);
    assert_eq!(report.blocking_suites, vec![SuiteKind::ThreatRegression]);
}

#[test]
fn a_blocked_benchmark_gate_blocks_release_even_with_every_suite_clean() {
    let report = evaluate_release(
        "build-1",
        &[clean_suite(SuiteKind::Deterministic)],
        Some(GateOutcome::Blocked),
        &HardwareReleaseCriteria::all_clear(),
    );
    assert_eq!(report.decision, ReleaseDecision::Blocked);
}

#[test]
fn a_warned_benchmark_outcome_does_not_block_release() {
    let report = evaluate_release(
        "build-1",
        &[clean_suite(SuiteKind::Deterministic)],
        Some(GateOutcome::Warned),
        &HardwareReleaseCriteria::all_clear(),
    );
    assert_eq!(report.decision, ReleaseDecision::Pass);
}

#[test]
fn recording_a_decision_and_verifying_completeness_finds_no_gap() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let audit = AuditLedger::new();

    let report = evaluate_release(
        "build-1",
        &[clean_suite(SuiteKind::Deterministic)],
        Some(GateOutcome::Pass),
        &HardwareReleaseCriteria::all_clear(),
    );
    record_release_decision(&monitor, &root, &audit, &report, 1_000).unwrap();

    let entries = audit.query(&monitor, &root, |_| true).unwrap();
    let gaps = verify_completeness(&entries, &["build-1".to_string()]);
    assert!(gaps.is_empty());
}

#[test]
fn a_build_with_no_recorded_decision_is_flagged_as_a_completeness_gap() {
    let gaps = verify_completeness(&[], &["build-1".to_string(), "build-2".to_string()]);
    assert_eq!(gaps, vec!["build-1".to_string(), "build-2".to_string()]);
}

#[test]
fn only_the_build_missing_a_decision_is_flagged_when_others_have_one() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let audit = AuditLedger::new();

    let report = evaluate_release(
        "build-1",
        &[clean_suite(SuiteKind::Deterministic)],
        Some(GateOutcome::Pass),
        &HardwareReleaseCriteria::all_clear(),
    );
    record_release_decision(&monitor, &root, &audit, &report, 1_000).unwrap();

    let entries = audit.query(&monitor, &root, |_| true).unwrap();
    let gaps = verify_completeness(&entries, &["build-1".to_string(), "build-2".to_string()]);
    assert_eq!(gaps, vec!["build-2".to_string()]);
}

#[test]
fn record_release_decision_requires_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();
    let audit = AuditLedger::new();
    let report = evaluate_release(
        "build-1",
        &[clean_suite(SuiteKind::Deterministic)],
        Some(GateOutcome::Pass),
        &HardwareReleaseCriteria::all_clear(),
    );

    let result = record_release_decision(&monitor, &read_only, &audit, &report, 1_000);
    assert!(matches!(result, Err(ReleaseGateError::Unauthorized)));
}

/// docs/998-roadmap.md M13: unmet hardware criteria block release the same way an unmet
/// suite or benchmark gate already do -- every *other* axis clean, only this one failing.
#[test]
fn unmet_hardware_criteria_blocks_release_even_with_every_suite_and_benchmark_clean() {
    let hardware = HardwareReleaseCriteria {
        image_build_reproducible: false,
        ..HardwareReleaseCriteria::all_clear()
    };
    let report = evaluate_release(
        "build-1",
        &[clean_suite(SuiteKind::Deterministic)],
        Some(GateOutcome::Pass),
        &hardware,
    );
    assert_eq!(report.decision, ReleaseDecision::Blocked);
    assert!(
        !report.hardware_criteria_met,
        "report must surface which axis actually blocked, not just the bare verdict"
    );
}

/// A reference platform simply missing from `boot_tested_platforms` -- never boot-tested at all,
/// not explicitly failed -- must be treated the same as a failing one: an untested platform is
/// not a passing one, per this milestone's own exit criterion naming both platforms explicitly.
#[test]
fn a_reference_platform_missing_from_the_boot_tested_list_blocks_release() {
    let hardware = HardwareReleaseCriteria {
        image_build_reproducible: true,
        boot_tested_platforms: vec![("x86_64".to_string(), true)], // aarch64 never mentioned
        staged_update_rollback_verified: true,
    };
    assert!(!hardware.is_met());
    let report = evaluate_release(
        "build-1",
        &[clean_suite(SuiteKind::Deterministic)],
        Some(GateOutcome::Pass),
        &hardware,
    );
    assert_eq!(report.decision, ReleaseDecision::Blocked);
}

/// A platform explicitly recorded as boot-test-*failed* (not merely absent) must also block.
#[test]
fn a_reference_platform_recorded_as_boot_test_failed_blocks_release() {
    let hardware = HardwareReleaseCriteria {
        image_build_reproducible: true,
        boot_tested_platforms: vec![("x86_64".to_string(), true), ("aarch64".to_string(), false)],
        staged_update_rollback_verified: true,
    };
    assert!(!hardware.is_met());
}

/// The real staged-update-and-rollback proof (docs/41 Phase 10's literal exit criterion) is its
/// own, independent hardware criterion -- not met blocks release even if both platforms
/// boot-tested fine and the image is reproducible.
#[test]
fn an_unverified_staged_update_rollback_blocks_release() {
    let hardware = HardwareReleaseCriteria {
        staged_update_rollback_verified: false,
        ..HardwareReleaseCriteria::all_clear()
    };
    assert!(!hardware.is_met());
    let report = evaluate_release(
        "build-1",
        &[clean_suite(SuiteKind::Deterministic)],
        Some(GateOutcome::Pass),
        &hardware,
    );
    assert_eq!(report.decision, ReleaseDecision::Blocked);
}

/// `HardwareReleaseCriteria::all_clear()` itself must actually satisfy `is_met()` -- otherwise
/// every other test in this file silently relies on a helper that's quietly broken.
#[test]
fn all_clear_hardware_criteria_is_met() {
    assert!(HardwareReleaseCriteria::all_clear().is_met());
}
