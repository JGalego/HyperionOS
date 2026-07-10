//! Mirrors every other crate in this workspace: the audit ledger's write
//! (and read) path is capability-gated, re-checked live against the
//! monitor. The lossy telemetry path is deliberately not gated — see
//! `hyperion_observability`'s crate doc comment.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_observability::{
    AuditAction, AuditLedger, AuditPayload, ObservabilityError, PrincipalRef,
};

#[test]
fn append_requires_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();
    let ledger = AuditLedger::new();

    let result = ledger.append(
        &monitor,
        &read_only,
        PrincipalRef::System,
        AuditAction::Grant,
        None,
        AuditPayload::Note("x".to_string()),
        1_000,
    );
    assert!(matches!(result, Err(ObservabilityError::Unauthorized)));
}

#[test]
fn query_requires_read_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let write_only = monitor
        .cap_derive(&root, RightsMask::WRITE, None, TrustBoundaryId(2))
        .unwrap();
    let ledger = AuditLedger::new();

    let result = ledger.query(&monitor, &write_only, |_| true);
    assert!(matches!(result, Err(ObservabilityError::Unauthorized)));
}

#[test]
fn revoking_a_token_blocks_further_access_re_checked_live() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let delegate = monitor
        .cap_derive(&root, RightsMask::all(), None, TrustBoundaryId(2))
        .unwrap();
    let ledger = AuditLedger::new();

    assert!(ledger
        .append(
            &monitor,
            &delegate,
            PrincipalRef::System,
            AuditAction::Grant,
            None,
            AuditPayload::Note("x".to_string()),
            1_000
        )
        .is_ok());

    monitor.cap_revoke(&delegate);

    assert!(matches!(
        ledger.append(
            &monitor,
            &delegate,
            PrincipalRef::System,
            AuditAction::Grant,
            None,
            AuditPayload::Note("y".to_string()),
            1_001
        ),
        Err(ObservabilityError::Unauthorized)
    ));
}
