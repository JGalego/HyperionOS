//! docs/34 §2/§6: the audit ledger is the only write path, hash-chained,
//! and a corruption is detected at its exact `seq`, never silently
//! repaired.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_observability::{
    AuditAction, AuditLedger, AuditPayload, PrincipalRef, VerificationReport,
};

fn setup() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    AuditLedger,
) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    (monitor, root, AuditLedger::new())
}

#[test]
fn a_freshly_appended_chain_verifies_intact() {
    let (monitor, root, ledger) = setup();
    for i in 0..5 {
        ledger
            .append(
                &monitor,
                &root,
                PrincipalRef::System,
                AuditAction::Grant,
                None,
                AuditPayload::Note(format!("entry {i}")),
                1_000 + i,
            )
            .unwrap();
    }
    assert_eq!(ledger.verify_chain(1, 5), VerificationReport::Intact);
}

#[test]
fn each_entry_hash_chains_from_the_previous() {
    let (monitor, root, ledger) = setup();
    let first = ledger
        .append(
            &monitor,
            &root,
            PrincipalRef::System,
            AuditAction::Grant,
            None,
            AuditPayload::Note("a".to_string()),
            1_000,
        )
        .unwrap();
    let second = ledger
        .append(
            &monitor,
            &root,
            PrincipalRef::System,
            AuditAction::Revoke,
            None,
            AuditPayload::Note("b".to_string()),
            1_001,
        )
        .unwrap();
    assert_eq!(second.prev_hash, first.entry_hash);
    assert_ne!(second.entry_hash, first.entry_hash);
}

#[test]
fn querying_filters_by_action_kind() {
    let (monitor, root, ledger) = setup();
    ledger
        .append(
            &monitor,
            &root,
            PrincipalRef::Agent(1),
            AuditAction::Grant,
            Some("web.research".to_string()),
            AuditPayload::Grant {
                capability_ref: "web.research".to_string(),
            },
            1_000,
        )
        .unwrap();
    ledger
        .append(
            &monitor,
            &root,
            PrincipalRef::Agent(1),
            AuditAction::ConsentChange,
            None,
            AuditPayload::ConsentChange { grant_id: 7 },
            1_001,
        )
        .unwrap();

    let grants = ledger
        .query(&monitor, &root, |e| e.action == AuditAction::Grant)
        .unwrap();
    assert_eq!(grants.len(), 1);
}

#[test]
fn an_explanation_record_is_embedded_verbatim_as_the_payload() {
    let (monitor, root, ledger) = setup();
    let store = hyperion_explainability::ExplanationStore::new();
    let id = store
        .begin(&monitor, &root, 1, 7, 42, "document.draft", vec![], 1_000)
        .unwrap();
    let record = store.get(&monitor, &root, id).unwrap().unwrap();

    let entry = ledger
        .append(
            &monitor,
            &root,
            PrincipalRef::Agent(42),
            AuditAction::ExplainRecord,
            None,
            AuditPayload::Explanation(record.clone()),
            1_000,
        )
        .unwrap();

    match entry.payload {
        AuditPayload::Explanation(embedded) => assert_eq!(embedded.id, record.id),
        _ => panic!("expected an embedded ExplanationRecord"),
    }
}

#[test]
fn verify_chain_validates_a_subrange_anchored_on_its_own_starting_prev_hash() {
    let (monitor, root, ledger) = setup();
    for i in 0..5 {
        ledger
            .append(
                &monitor,
                &root,
                PrincipalRef::System,
                AuditAction::Grant,
                None,
                AuditPayload::Note(format!("entry {i}")),
                1_000 + i,
            )
            .unwrap();
    }

    // A subrange not starting at seq 1 must anchor on that entry's own
    // recorded `prev_hash`, not the genesis hash.
    assert_eq!(ledger.verify_chain(3, 5), VerificationReport::Intact);
}

#[test]
fn verifying_an_empty_range_reports_empty() {
    let (_monitor, _root, ledger) = setup();
    assert_eq!(ledger.verify_chain(1, 5), VerificationReport::Empty);
}
