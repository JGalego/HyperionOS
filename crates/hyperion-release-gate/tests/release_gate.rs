//! docs/35 §1's `ReleaseGate.evaluate`: every suite must be non-blocking
//! and the benchmark gate must not be `Blocked`; the threat-regression
//! suite's provenance distinction is the one suite-specific exception to
//! "any failure blocks."

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_observability::AuditLedger;
use hyperion_release_gate::{
    evaluate_release, record_release_decision, verify_completeness, GateOutcome, ReleaseDecision,
    ReleaseGateError, SuiteKind, SuiteReport,
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
    let report = evaluate_release("build-1", &suites, Some(GateOutcome::Pass));
    assert_eq!(report.decision, ReleaseDecision::Pass);
    assert!(report.blocking_suites.is_empty());
}

#[test]
fn a_deterministic_suite_failure_blocks_release() {
    let mut deterministic = clean_suite(SuiteKind::Deterministic);
    deterministic.failed = 1;
    let report = evaluate_release("build-1", &[deterministic], Some(GateOutcome::Pass));
    assert_eq!(report.decision, ReleaseDecision::Blocked);
    assert_eq!(report.blocking_suites, vec![SuiteKind::Deterministic]);
}

#[test]
fn a_never_tested_threat_gap_does_not_block_release() {
    let mut threat = clean_suite(SuiteKind::ThreatRegression);
    threat.failed = 1; // a failure exists, but it's not in regressed_previously_mitigated
    let report = evaluate_release("build-1", &[threat], Some(GateOutcome::Pass));
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
    let report = evaluate_release("build-1", &[threat], Some(GateOutcome::Pass));
    assert_eq!(report.decision, ReleaseDecision::Blocked);
    assert_eq!(report.blocking_suites, vec![SuiteKind::ThreatRegression]);
}

#[test]
fn a_blocked_benchmark_gate_blocks_release_even_with_every_suite_clean() {
    let report = evaluate_release(
        "build-1",
        &[clean_suite(SuiteKind::Deterministic)],
        Some(GateOutcome::Blocked),
    );
    assert_eq!(report.decision, ReleaseDecision::Blocked);
}

#[test]
fn a_warned_benchmark_outcome_does_not_block_release() {
    let report = evaluate_release(
        "build-1",
        &[clean_suite(SuiteKind::Deterministic)],
        Some(GateOutcome::Warned),
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
    );

    let result = record_release_decision(&monitor, &read_only, &audit, &report, 1_000);
    assert!(matches!(result, Err(ReleaseGateError::Unauthorized)));
}
