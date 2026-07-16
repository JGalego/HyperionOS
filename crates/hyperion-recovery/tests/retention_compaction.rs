//! This crate's own named "retention classes, compaction, and pinning enforcement beyond a
//! boolean flag" gap, real for the first time: `RecoveryService::compact` evicts an unpinned
//! `RecoveryPoint` (and its snapshot) once its age reaches a caller-chosen `retention_secs` --
//! but never one still needed by a live (`InFlight`/`Committed`) `ActionRecord`, and never one
//! that's been pinned, matching `pin`/`unpin`'s own long-standing but previously-unread promise.

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

const RETENTION_SECS: u64 = 86_400;

#[test]
fn a_point_within_its_retention_window_is_never_evicted_regardless_of_pin_state() {
    let (monitor, root, recovery, graph) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "a"}),
        )
        .unwrap();
    let rp = recovery
        .recovery_point_create(&monitor, &root, Trigger::UserRequested, &[node], 1_000)
        .unwrap();

    let evicted = recovery.compact(1_000 + RETENTION_SECS - 1, RETENTION_SECS);
    assert_eq!(evicted, 0);
    assert!(recovery.recovery_point(rp).is_some());
}

#[test]
fn an_unpinned_point_past_retention_with_no_referencing_action_is_evicted() {
    let (monitor, root, recovery, graph) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "a"}),
        )
        .unwrap();
    let rp = recovery
        .recovery_point_create(&monitor, &root, Trigger::UserRequested, &[node], 1_000)
        .unwrap();

    let evicted = recovery.compact(1_000 + RETENTION_SECS, RETENTION_SECS);
    assert_eq!(evicted, 1);
    assert!(
        recovery.recovery_point(rp).is_none(),
        "a real eviction must really remove the recovery point, not just report a count"
    );
}

#[test]
fn a_pinned_point_past_retention_survives() {
    let (monitor, root, recovery, graph) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "a"}),
        )
        .unwrap();
    let rp = recovery
        .recovery_point_create(&monitor, &root, Trigger::UserRequested, &[node], 1_000)
        .unwrap();
    recovery.pin(&monitor, &root, rp).unwrap();

    let evicted = recovery.compact(1_000 + RETENTION_SECS, RETENTION_SECS);
    assert_eq!(
        evicted, 0,
        "pin must really protect a point from eviction, matching its own long-standing promise"
    );
    assert!(recovery.recovery_point(rp).is_some());
}

#[test]
fn an_unpinned_point_backing_a_committed_action_is_never_evicted_and_undo_still_works() {
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

    let evicted = recovery.compact(1_000 + RETENTION_SECS, RETENTION_SECS);
    assert_eq!(
        evicted, 0,
        "a point backing a still-Committed action must never be evicted -- undo would break"
    );

    let receipt = recovery
        .undo(&monitor, &root, UndoScope::SingleAction(action))
        .unwrap();
    assert!(matches!(receipt, UndoReceipt::Targeted { .. }));
    let restored = graph.get(&monitor, &root, node).unwrap();
    assert_eq!(restored.metadata["text"], serde_json::json!("original"));
}

#[test]
fn an_unpinned_point_backing_only_undone_actions_is_evicted_past_retention() {
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

    let evicted = recovery.compact(1_000 + RETENTION_SECS, RETENTION_SECS);
    assert_eq!(
        evicted, 1,
        "an Undone action's recovery point is no longer needed -- redo restores from its own \
         separate redo_snapshots, not this point"
    );
    assert!(recovery.recovery_point(rp).is_none());
}
