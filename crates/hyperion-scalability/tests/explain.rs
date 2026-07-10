//! docs/37 §3's `apply_and_explain`: the audit notice is written as a
//! real, tamper-evident `hyperion-observability` entry.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_observability::{AuditAction, AuditLedger, AuditPayload, PrincipalRef};
use hyperion_scalability::{
    apply_and_explain, DegradationOutcome, DegradationPlan, ScalabilityError, Substitution,
};

#[test]
fn a_degradation_plan_is_recorded_verbatim_in_the_audit_ledger() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let audit = AuditLedger::new();
    let plan = DegradationPlan {
        capability_ref: "vision.generate".to_string(),
        outcome: DegradationOutcome::Substituted {
            substitution: Substitution::Disable,
        },
        notice: "vision.generate disabled on this device".to_string(),
    };

    apply_and_explain(&monitor, &root, &audit, PrincipalRef::System, &plan, 1_000).unwrap();

    let entries = audit
        .query(&monitor, &root, |e| e.action == AuditAction::AdminOverride)
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].target, Some("vision.generate".to_string()));
    match &entries[0].payload {
        AuditPayload::Note(note) => assert_eq!(note, &plan.notice),
        other => panic!("expected a Note payload, got {other:?}"),
    }
}

#[test]
fn apply_and_explain_requires_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();
    let audit = AuditLedger::new();
    let plan = DegradationPlan {
        capability_ref: "x".to_string(),
        outcome: DegradationOutcome::FullFidelity,
        notice: "n".to_string(),
    };

    let result = apply_and_explain(
        &monitor,
        &read_only,
        &audit,
        PrincipalRef::System,
        &plan,
        1_000,
    );
    assert!(matches!(result, Err(ScalabilityError::Unauthorized)));
}
