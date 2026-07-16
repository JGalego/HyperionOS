//! docs/33 §4's `UndoScope::Session`/`UndoScope::Goal`, closed for real:
//! `RecoveryService::record_action_started_with_scope` tags an `ActionRecord` with a real
//! session/goal id, and `undo(UndoScope::Session(..))`/`undo(UndoScope::Goal(..))` really scope
//! to only the actions tagged with that exact id -- distinct from `UndoScope::AgentRun`/`Global`.

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
fn undo_scope_session_undoes_only_actions_tagged_with_that_real_session_id() {
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
    let action_session_1 = recovery.record_action_started_with_scope(
        rp,
        vec![a],
        None,
        Some(1),
        None,
        "session 1's edit",
        1_000,
    );
    let action_session_2 = recovery.record_action_started_with_scope(
        rp,
        vec![b],
        None,
        Some(2),
        None,
        "session 2's edit",
        1_000,
    );
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
    recovery.record_action_committed(action_session_1).unwrap();
    recovery.record_action_committed(action_session_2).unwrap();

    let receipt = recovery
        .undo(&monitor, &root, UndoScope::Session(1))
        .unwrap();
    assert!(
        matches!(receipt, UndoReceipt::Targeted { undone_actions } if undone_actions == vec![action_session_1])
    );

    assert_eq!(
        graph.get(&monitor, &root, a).unwrap().metadata["text"],
        serde_json::json!("a1"),
        "session 1's own action must be undone"
    );
    assert_eq!(
        graph.get(&monitor, &root, b).unwrap().metadata["text"],
        serde_json::json!("b2"),
        "session 2's action must be untouched by a scope naming only session 1"
    );
}

#[test]
fn undo_scope_goal_undoes_only_actions_tagged_with_that_real_goal_id() {
    let (monitor, root, recovery, graph) = setup();
    let goal_a = hyperion_storage::ObjectId(100);
    let goal_b = hyperion_storage::ObjectId(200);

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
    let action_goal_a = recovery.record_action_started_with_scope(
        rp,
        vec![a],
        None,
        None,
        Some(goal_a),
        "goal a's edit",
        1_000,
    );
    let action_goal_b = recovery.record_action_started_with_scope(
        rp,
        vec![b],
        None,
        None,
        Some(goal_b),
        "goal b's edit",
        1_000,
    );
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
    recovery.record_action_committed(action_goal_a).unwrap();
    recovery.record_action_committed(action_goal_b).unwrap();

    let receipt = recovery
        .undo(&monitor, &root, UndoScope::Goal(goal_b))
        .unwrap();
    assert!(
        matches!(receipt, UndoReceipt::Targeted { undone_actions } if undone_actions == vec![action_goal_b])
    );

    assert_eq!(
        graph.get(&monitor, &root, a).unwrap().metadata["text"],
        serde_json::json!("a2"),
        "goal a's own action must be untouched by a scope naming only goal b"
    );
    assert_eq!(
        graph.get(&monitor, &root, b).unwrap().metadata["text"],
        serde_json::json!("b1"),
        "goal b's own action must be undone"
    );
}

#[test]
fn an_untagged_action_is_never_matched_by_a_session_or_goal_scope() {
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
    // The plain, untagged path -- record_action_started, not record_action_started_with_scope.
    let action = recovery.record_action_started(rp, vec![node], None, "untagged edit", 1_000);
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
        .undo(&monitor, &root, UndoScope::Session(1))
        .unwrap();
    assert!(
        matches!(receipt, UndoReceipt::NothingToUndo),
        "an action recorded with no session id must never match any UndoScope::Session, got: \
         {receipt:?}"
    );
}
