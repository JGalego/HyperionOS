//! docs/16 §10's own real motivating case for `RecoveryService::expire`: sealing a `Committed`
//! action so it can never be undone/redone again, distinct from `Aborted` (never took effect) and
//! `Undone` (reverted).

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_recovery::{
    ActionStatus, RecoveryError, RecoveryService, Trigger, UndoReceipt, UndoScope,
};

fn setup() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    RecoveryService,
    Arc<KnowledgeGraph>,
) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let recovery = RecoveryService::new(graph.clone());
    (monitor, root, recovery, graph)
}

#[test]
fn expiring_a_committed_action_makes_it_permanently_not_undoable() {
    let (monitor, root, recovery, graph) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "original"}),
        )
        .unwrap();
    let rp = recovery
        .recovery_point_create(&monitor, &root, Trigger::UserRequested, &[node], 1_000)
        .unwrap();
    let action = recovery.record_action_started(rp, vec![node], None, "edit note", 1_000);
    recovery.record_action_committed(action).unwrap();

    recovery.expire(action).unwrap();

    let result = recovery
        .undo(&monitor, &root, UndoScope::SingleAction(action))
        .unwrap();
    assert!(matches!(result, UndoReceipt::NothingToUndo));
}

#[test]
fn expiring_an_in_flight_action_is_rejected() {
    let (monitor, root, recovery, graph) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "original"}),
        )
        .unwrap();
    let rp = recovery
        .recovery_point_create(&monitor, &root, Trigger::UserRequested, &[node], 1_000)
        .unwrap();
    let action = recovery.record_action_started(rp, vec![node], None, "edit note", 1_000);

    let result = recovery.expire(action);
    assert!(matches!(result, Err(RecoveryError::ActionNotCommitted)));
}

#[test]
fn expiring_an_already_undone_action_is_rejected() {
    let (monitor, root, recovery, graph) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "original"}),
        )
        .unwrap();
    let rp = recovery
        .recovery_point_create(&monitor, &root, Trigger::UserRequested, &[node], 1_000)
        .unwrap();
    let action = recovery.record_action_started(rp, vec![node], None, "edit note", 1_000);
    graph
        .put_node(
            &monitor,
            &root,
            Some(node),
            "Note",
            None,
            serde_json::json!({"text": "edited"}),
        )
        .unwrap();
    recovery.record_action_committed(action).unwrap();
    recovery
        .undo(&monitor, &root, UndoScope::SingleAction(action))
        .unwrap();

    let result = recovery.expire(action);
    assert!(matches!(result, Err(RecoveryError::ActionNotCommitted)));
}

#[test]
fn expiring_an_unknown_action_fails() {
    let (_monitor, _root, recovery, _graph) = setup();
    let result = recovery.expire(999);
    assert!(matches!(result, Err(RecoveryError::NoSuchAction)));
}

#[test]
fn an_expired_actions_real_effects_still_block_an_earlier_actions_undo_as_a_real_conflict() {
    let (monitor, root, recovery, graph) = setup();
    let a = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "original"}),
        )
        .unwrap();

    let rp0 = recovery
        .recovery_point_create(&monitor, &root, Trigger::UserRequested, &[a], 500)
        .unwrap();
    let action0 = recovery.record_action_started(rp0, vec![a], None, "first edit", 500);
    graph
        .put_node(
            &monitor,
            &root,
            Some(a),
            "Note",
            None,
            serde_json::json!({"text": "b"}),
        )
        .unwrap();
    recovery.record_action_committed(action0).unwrap();

    let rp1 = recovery
        .recovery_point_create(&monitor, &root, Trigger::UserRequested, &[a], 1_000)
        .unwrap();
    let action1 = recovery.record_action_started(rp1, vec![a], None, "second edit", 1_000);
    graph
        .put_node(
            &monitor,
            &root,
            Some(a),
            "Note",
            None,
            serde_json::json!({"text": "c"}),
        )
        .unwrap();
    recovery.record_action_committed(action1).unwrap();
    recovery.expire(action1).unwrap();
    assert_eq!(
        recovery
            .action_records()
            .into_iter()
            .find(|r| r.action_id == action1)
            .unwrap()
            .status,
        ActionStatus::Expired
    );

    // action1's own real effects on `a` happened after action0's own recovery point -- an
    // Expired action must still count as a genuine conflict here (unlike Aborted/Undone, which
    // undo() explicitly excludes), so undoing action0 alone must surface it rather than silently
    // clobbering action1's real, permanently-sealed edit.
    let result = recovery
        .undo(&monitor, &root, UndoScope::SingleAction(action0))
        .unwrap();
    assert!(matches!(result, UndoReceipt::NeedsConfirmation { .. }));
}
