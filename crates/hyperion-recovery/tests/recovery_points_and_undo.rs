//! docs/33 §5: recovery points capture pre-action state, `undo` restores
//! directly when nothing else touched the same objects since, and
//! surfaces conflicts (never a silent overwrite) otherwise.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_recovery::{RecoveryService, Trigger, UndoReceipt, UndoScope};

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
fn undoing_a_committed_action_restores_the_prior_metadata() {
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

    let receipt = recovery
        .undo(&monitor, &root, UndoScope::SingleAction(action))
        .unwrap();
    assert!(matches!(receipt, UndoReceipt::Targeted { .. }));

    let restored = graph.get(&monitor, &root, node).unwrap();
    assert_eq!(restored.metadata["text"], serde_json::json!("original"));
}

#[test]
fn undoing_twice_is_idempotent_nothing_to_undo() {
    let (monitor, root, recovery, graph) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "v1"}),
        )
        .unwrap();
    let rp = recovery
        .recovery_point_create(&monitor, &root, Trigger::UserRequested, &[node], 1_000)
        .unwrap();
    let action = recovery.record_action_started(rp, vec![node], None, "edit", 1_000);
    graph
        .put_node(
            &monitor,
            &root,
            Some(node),
            "Note",
            None,
            serde_json::json!({"text": "v2"}),
        )
        .unwrap();
    recovery.record_action_committed(action).unwrap();

    recovery
        .undo(&monitor, &root, UndoScope::SingleAction(action))
        .unwrap();
    let second = recovery
        .undo(&monitor, &root, UndoScope::SingleAction(action))
        .unwrap();
    assert!(matches!(second, UndoReceipt::NothingToUndo));
}

#[test]
fn a_conflicting_later_edit_by_something_outside_scope_blocks_a_silent_restore() {
    let (monitor, root, recovery, graph) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "v1"}),
        )
        .unwrap();

    let rp = recovery
        .recovery_point_create(&monitor, &root, Trigger::UserRequested, &[node], 1_000)
        .unwrap();
    let action_a = recovery.record_action_started(rp, vec![node], None, "agent A edits", 1_000);
    graph
        .put_node(
            &monitor,
            &root,
            Some(node),
            "Note",
            None,
            serde_json::json!({"text": "v2-by-a"}),
        )
        .unwrap();
    recovery.record_action_committed(action_a).unwrap();

    // A second, unrelated action (outside the scope we're about to undo)
    // touches the same object afterward.
    let rp2 = recovery
        .recovery_point_create(&monitor, &root, Trigger::UserRequested, &[node], 1_010)
        .unwrap();
    let action_b = recovery.record_action_started(rp2, vec![node], None, "agent B edits", 1_010);
    graph
        .put_node(
            &monitor,
            &root,
            Some(node),
            "Note",
            None,
            serde_json::json!({"text": "v3-by-b"}),
        )
        .unwrap();
    recovery.record_action_committed(action_b).unwrap();

    let receipt = recovery
        .undo(&monitor, &root, UndoScope::SingleAction(action_a))
        .unwrap();
    assert!(
        matches!(receipt, UndoReceipt::NeedsConfirmation { conflicting_objects } if conflicting_objects.contains(&node))
    );

    // Nothing changed — the conflicting edit was never silently overwritten.
    let current = graph.get(&monitor, &root, node).unwrap();
    assert_eq!(current.metadata["text"], serde_json::json!("v3-by-b"));
}

#[test]
fn undo_scope_global_undoes_every_action_sharing_a_recovery_point() {
    let (monitor, root, recovery, graph) = setup();
    let a = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "a1"}),
        )
        .unwrap();
    let b = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "b1"}),
        )
        .unwrap();

    let rp = recovery
        .recovery_point_create(&monitor, &root, Trigger::UserRequested, &[a, b], 1_000)
        .unwrap();
    let action_a = recovery.record_action_started(rp, vec![a], None, "edit a", 1_000);
    let action_b = recovery.record_action_started(rp, vec![b], None, "edit b", 1_000);
    graph
        .put_node(
            &monitor,
            &root,
            Some(a),
            "Note",
            None,
            serde_json::json!({"text": "a2"}),
        )
        .unwrap();
    graph
        .put_node(
            &monitor,
            &root,
            Some(b),
            "Note",
            None,
            serde_json::json!({"text": "b2"}),
        )
        .unwrap();
    recovery.record_action_committed(action_a).unwrap();
    recovery.record_action_committed(action_b).unwrap();

    let receipt = recovery
        .undo(&monitor, &root, UndoScope::Global(rp))
        .unwrap();
    assert!(
        matches!(receipt, UndoReceipt::Targeted { undone_actions } if undone_actions.len() == 2)
    );

    assert_eq!(
        graph.get(&monitor, &root, a).unwrap().metadata["text"],
        serde_json::json!("a1")
    );
    assert_eq!(
        graph.get(&monitor, &root, b).unwrap().metadata["text"],
        serde_json::json!("b1")
    );
}

#[test]
fn a_recovery_point_over_a_not_yet_created_object_cannot_undo_its_creation() {
    let (monitor, root, recovery, graph) = setup();
    let future_id = hyperion_storage::ObjectId(999);
    let rp = recovery
        .recovery_point_create(&monitor, &root, Trigger::UserRequested, &[future_id], 1_000)
        .unwrap();
    let action = recovery.record_action_started(rp, vec![future_id], None, "create", 1_000);
    let created = graph
        .put_node(
            &monitor,
            &root,
            Some(future_id),
            "Note",
            None,
            serde_json::json!({"text": "brand new"}),
        )
        .unwrap();
    recovery.record_action_committed(action).unwrap();

    recovery
        .undo(&monitor, &root, UndoScope::SingleAction(action))
        .unwrap();

    // The object still exists — creation cannot be undone (no node-delete
    // in the Knowledge Graph), documented as a limitation.
    let still_there = graph.get(&monitor, &root, created).unwrap();
    assert_eq!(still_there.metadata["text"], serde_json::json!("brand new"));
}

#[test]
fn pin_and_unpin_round_trip() {
    let (monitor, root, recovery, graph) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "x"}),
        )
        .unwrap();
    let rp = recovery
        .recovery_point_create(&monitor, &root, Trigger::Automatic, &[node], 1_000)
        .unwrap();

    recovery.pin(&monitor, &root, rp).unwrap();
    assert!(recovery.recovery_point(rp).unwrap().pinned);
    recovery.unpin(&monitor, &root, rp).unwrap();
    assert!(!recovery.recovery_point(rp).unwrap().pinned);
}
