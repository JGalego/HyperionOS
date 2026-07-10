//! Mirrors every other crate in this workspace: every call is capability-
//! gated, re-checked live against the monitor.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_recovery::{RecoveryError, RecoveryService, Trigger, UndoScope};

#[test]
fn recovery_point_create_requires_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let recovery = RecoveryService::new(graph);

    let result =
        recovery.recovery_point_create(&monitor, &read_only, Trigger::UserRequested, &[], 1_000);
    assert!(matches!(result, Err(RecoveryError::Unauthorized)));
}

#[test]
fn undo_requires_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let recovery = RecoveryService::new(graph);

    let result = recovery.undo(&monitor, &read_only, UndoScope::SingleAction(1));
    assert!(matches!(result, Err(RecoveryError::Unauthorized)));
}

#[test]
fn revoking_a_token_blocks_further_access_re_checked_live() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let delegate = monitor
        .cap_derive(&root, RightsMask::all(), None, TrustBoundaryId(2))
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let recovery = RecoveryService::new(graph);

    assert!(recovery
        .recovery_point_create(&monitor, &delegate, Trigger::UserRequested, &[], 1_000)
        .is_ok());

    monitor.cap_revoke(&delegate);

    assert!(matches!(
        recovery.recovery_point_create(&monitor, &delegate, Trigger::UserRequested, &[], 1_001),
        Err(RecoveryError::Unauthorized)
    ));
}
