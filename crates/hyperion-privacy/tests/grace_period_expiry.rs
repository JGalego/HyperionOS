//! docs/16 §10's "soft-deletes honor a grace period before cryptographic shredding" real timer:
//! `expire_lapsed_soft_deletes` seals a soft-delete's `ActionRecord` for real once its grace
//! period has passed, after which it can never be undone again.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_privacy::{erase, expire_lapsed_soft_deletes, ErasureMode};
use hyperion_recovery::{RecoveryService, Trigger, UndoScope};

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    Arc<KnowledgeGraph>,
    RecoveryService,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let recovery = RecoveryService::new(graph.clone());
    (dir, monitor, root, graph, recovery)
}

const GRACE_PERIOD_SECS: u64 = 86_400; // one day, an arbitrary but realistic real-world default

#[test]
fn a_soft_delete_within_its_grace_period_is_not_expired() {
    let (_dir, monitor, root, graph, recovery) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "sensitive"}),
        )
        .unwrap();
    let receipt = erase(
        &monitor,
        &root,
        &graph,
        &recovery,
        &[node],
        ErasureMode::SoftDelete,
        1_000,
    )
    .unwrap();
    let action_id = receipt.grace_period_action.unwrap();

    // Well within the grace period.
    let expired = expire_lapsed_soft_deletes(
        &monitor,
        &root,
        &graph,
        &recovery,
        1_000 + GRACE_PERIOD_SECS - 1,
        GRACE_PERIOD_SECS,
    );
    assert!(expired.is_empty());

    // Still really undoable.
    recovery
        .undo(&monitor, &root, UndoScope::SingleAction(action_id))
        .unwrap();
    let restored = graph.get(&monitor, &root, node).unwrap();
    assert_eq!(restored.metadata["text"], serde_json::json!("sensitive"));
}

#[test]
fn a_soft_delete_past_its_grace_period_is_expired_and_can_never_be_undone_again() {
    let (_dir, monitor, root, graph, recovery) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "sensitive"}),
        )
        .unwrap();
    let receipt = erase(
        &monitor,
        &root,
        &graph,
        &recovery,
        &[node],
        ErasureMode::SoftDelete,
        1_000,
    )
    .unwrap();
    let action_id = receipt.grace_period_action.unwrap();

    let now = 1_000 + GRACE_PERIOD_SECS;
    let expired =
        expire_lapsed_soft_deletes(&monitor, &root, &graph, &recovery, now, GRACE_PERIOD_SECS);
    assert_eq!(expired, vec![action_id]);

    let result = recovery
        .undo(&monitor, &root, UndoScope::SingleAction(action_id))
        .unwrap();
    assert!(
        matches!(result, hyperion_recovery::UndoReceipt::NothingToUndo),
        "an expired action must never be undoable again, exactly like CryptoShred's own \
         no-grace-period floor"
    );

    let shredded = graph.get(&monitor, &root, node);
    assert!(
        matches!(
            shredded,
            Err(hyperion_knowledge_graph::GraphError::NotFound)
        ),
        "a lapsed soft-delete must really shred the object, matching CryptoShred's own \
         genuine tombstone, not merely leave an overwritten-but-still-readable placeholder \
         forever; got: {shredded:?}"
    );
}

#[test]
fn sweeping_twice_only_expires_each_action_once() {
    let (_dir, monitor, root, graph, recovery) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "sensitive"}),
        )
        .unwrap();
    let receipt = erase(
        &monitor,
        &root,
        &graph,
        &recovery,
        &[node],
        ErasureMode::SoftDelete,
        1_000,
    )
    .unwrap();
    let action_id = receipt.grace_period_action.unwrap();

    let now = 1_000 + GRACE_PERIOD_SECS;
    let first_sweep =
        expire_lapsed_soft_deletes(&monitor, &root, &graph, &recovery, now, GRACE_PERIOD_SECS);
    assert_eq!(first_sweep, vec![action_id]);

    let second_sweep =
        expire_lapsed_soft_deletes(&monitor, &root, &graph, &recovery, now, GRACE_PERIOD_SECS);
    assert!(
        second_sweep.is_empty(),
        "an already-Expired action must never be re-expired (it's no longer Committed)"
    );
}

#[test]
fn a_crypto_shred_erasure_has_nothing_for_the_sweep_to_touch() {
    let (_dir, monitor, root, graph, recovery) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "sensitive"}),
        )
        .unwrap();
    erase(
        &monitor,
        &root,
        &graph,
        &recovery,
        &[node],
        ErasureMode::CryptoShred,
        1_000,
    )
    .unwrap();

    let expired = expire_lapsed_soft_deletes(
        &monitor,
        &root,
        &graph,
        &recovery,
        1_000 + GRACE_PERIOD_SECS,
        GRACE_PERIOD_SECS,
    );
    assert!(
        expired.is_empty(),
        "CryptoShred journals nothing, so the sweep must find nothing to expire"
    );
}

#[test]
fn an_unrelated_recovery_action_is_never_swept_even_if_old_enough() {
    let (_dir, monitor, root, graph, recovery) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "unrelated"}),
        )
        .unwrap();

    // A real ActionRecord from some other, unrelated subsystem -- not tagged with this crate's
    // own soft-delete note -- must never be swept by this crate's own grace-period sweep.
    let point_id = recovery
        .recovery_point_create(&monitor, &root, Trigger::UserRequested, &[node], 1_000)
        .unwrap();
    let action_id =
        recovery.record_action_started(point_id, vec![node], None, "some other subsystem", 1_000);
    recovery.record_action_committed(action_id).unwrap();

    let expired = expire_lapsed_soft_deletes(
        &monitor,
        &root,
        &graph,
        &recovery,
        1_000 + GRACE_PERIOD_SECS,
        GRACE_PERIOD_SECS,
    );
    assert!(expired.is_empty());

    // Still really undoable -- the sweep must not have touched it at all.
    recovery
        .undo(&monitor, &root, UndoScope::SingleAction(action_id))
        .unwrap();
}
