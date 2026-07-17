//! Mirrors every other crate in this workspace: every call is capability-
//! gated, re-checked live against the monitor.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_privacy::{erase, ConsentLedger, DataScope, ErasureMode, PrivacyError};
use hyperion_recovery::RecoveryService;

#[test]
fn consent_request_requires_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();
    let ledger = ConsentLedger::new();
    let device_key = Keystore::ephemeral();

    let result = ledger.request(
        &monitor,
        &read_only,
        1,
        DataScope::Domain("notes".to_string()),
        "x",
        None,
        1_000,
        &device_key,
    );
    assert!(matches!(result, Err(PrivacyError::Unauthorized)));
}

#[test]
fn erase_requires_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let recovery = RecoveryService::new(graph.clone());

    let result = erase(
        &monitor,
        &read_only,
        &graph,
        &recovery,
        &[],
        ErasureMode::SoftDelete,
        1_000,
    );
    assert!(matches!(result, Err(PrivacyError::Unauthorized)));
}

#[test]
fn revoking_a_token_blocks_further_access_re_checked_live() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let delegate = monitor
        .cap_derive(&root, RightsMask::all(), None, TrustBoundaryId(2))
        .unwrap();
    let ledger = ConsentLedger::new();
    let device_key = Keystore::ephemeral();

    assert!(ledger
        .request(
            &monitor,
            &delegate,
            1,
            DataScope::Domain("notes".to_string()),
            "x",
            None,
            1_000,
            &device_key,
        )
        .is_ok());

    monitor.cap_revoke(&delegate);

    assert!(matches!(
        ledger.request(
            &monitor,
            &delegate,
            1,
            DataScope::Domain("notes".to_string()),
            "x",
            None,
            1_001,
            &device_key,
        ),
        Err(PrivacyError::Unauthorized)
    ));
}
