//! docs/33's `redo(scope)`: re-applies an already-undone action's real
//! captured effects, gated by the same "surface conflicts, never silently
//! overwrite concurrent work" rule `undo` already enforces.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_recovery::{RecoveryService, RedoReceipt, Trigger, UndoReceipt, UndoScope};

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
fn redoing_an_undone_action_restores_the_undone_edit() {
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

    let undo_receipt = recovery
        .undo(&monitor, &root, UndoScope::SingleAction(action))
        .unwrap();
    assert!(matches!(undo_receipt, UndoReceipt::Targeted { .. }));
    assert_eq!(
        graph.get(&monitor, &root, node).unwrap().metadata["text"],
        serde_json::json!("original")
    );

    let redo_receipt = recovery
        .redo(&monitor, &root, UndoScope::SingleAction(action))
        .unwrap();
    match redo_receipt {
        RedoReceipt::Targeted { redone_actions } => assert_eq!(redone_actions, vec![action]),
        other => panic!("expected Targeted, got {other:?}"),
    }
    assert_eq!(
        graph.get(&monitor, &root, node).unwrap().metadata["text"],
        serde_json::json!("edited"),
        "redo should bring back the action's real committed effect, not the pre-action state"
    );
}

#[test]
fn redoing_an_action_that_was_never_undone_reports_nothing_to_redo() {
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

    let receipt = recovery
        .redo(&monitor, &root, UndoScope::SingleAction(action))
        .unwrap();
    assert!(matches!(receipt, RedoReceipt::NothingToRedo));
}

#[test]
fn a_conflicting_commit_after_undo_blocks_a_silent_redo() {
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

    // Something else commits a real, independent edit to the same object
    // after the undo -- redoing the original action must not silently
    // clobber it.
    let rp2 = recovery
        .recovery_point_create(&monitor, &root, Trigger::UserRequested, &[node], 2_000)
        .unwrap();
    let other_action =
        recovery.record_action_started(rp2, vec![node], None, "someone else's edit", 2_000);
    graph
        .put_node(
            &monitor,
            &root,
            Some(node),
            "Note",
            None,
            serde_json::json!({"text": "someone else's edit"}),
        )
        .unwrap();
    recovery.record_action_committed(other_action).unwrap();

    let receipt = recovery
        .redo(&monitor, &root, UndoScope::SingleAction(action))
        .unwrap();
    match receipt {
        RedoReceipt::NeedsConfirmation {
            conflicting_objects,
        } => {
            assert_eq!(conflicting_objects, vec![node]);
        }
        other => panic!("expected NeedsConfirmation, got {other:?}"),
    }
    assert_eq!(
        graph.get(&monitor, &root, node).unwrap().metadata["text"],
        serde_json::json!("someone else's edit"),
        "a blocked redo must never overwrite the conflicting data"
    );
}
