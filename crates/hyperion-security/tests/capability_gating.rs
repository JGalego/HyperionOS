//! Mirrors every other crate in this workspace: every call is capability-
//! gated, re-checked live against the monitor.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_recovery::RecoveryService;
use hyperion_security::{assess_and_prepare, PendingAction, SecurityError, SensitivityHint};

fn trivial_action() -> PendingAction {
    PendingAction {
        action_id: 1,
        object_refs: Vec::new(),
        scope_size: 1,
        reversible: true,
        sensitivity: SensitivityHint::Public,
        intent_confidence: 1.0,
        corroboration: 1.0,
        provenance: None,
    }
}

#[test]
fn assess_and_prepare_requires_exec_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let write_only = monitor
        .cap_derive(&root, RightsMask::WRITE, None, TrustBoundaryId(2))
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let recovery = RecoveryService::new(graph.clone());

    let result = assess_and_prepare(
        &monitor,
        &write_only,
        &graph,
        &recovery,
        &trivial_action(),
        1_000,
    );
    assert!(matches!(result, Err(SecurityError::Unauthorized)));
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
    let recovery = RecoveryService::new(graph.clone());

    assert!(assess_and_prepare(
        &monitor,
        &delegate,
        &graph,
        &recovery,
        &trivial_action(),
        1_000
    )
    .is_ok());

    monitor.cap_revoke(&delegate);

    assert!(matches!(
        assess_and_prepare(
            &monitor,
            &delegate,
            &graph,
            &recovery,
            &trivial_action(),
            1_001
        ),
        Err(SecurityError::Unauthorized)
    ));
}
