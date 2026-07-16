//! docs/998-roadmap.md's Self-Sustaining pillar: `hyperion-recovery` used to be purely
//! reactive, with no mechanism connecting a rollback's cause to a future decision.
//! `restore_to_with_cause` really remembers why a rollback happened (in a real, wired
//! `MemoryEngine`'s Procedural tier); `rollback_causes` really queries that history back.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_memory::MemoryEngine;
use hyperion_recovery::{RecoveryService, RollbackCause, Trigger};

fn setup_with_memory() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    RecoveryService,
    Arc<KnowledgeGraph>,
) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let memory = Arc::new(MemoryEngine::new(graph.clone()));
    let recovery = RecoveryService::new_with_memory(graph.clone(), Some(memory));
    (monitor, root, recovery, graph)
}

#[test]
fn without_memory_wired_rollback_causes_is_always_empty_but_restore_still_works() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let recovery = RecoveryService::new(graph.clone());
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
        .recovery_point_create(&monitor, &root, Trigger::PreUpdate, &[node], 1_000)
        .unwrap();

    recovery
        .restore_to_with_cause(
            &monitor,
            &root,
            rp,
            "my-subject",
            RollbackCause {
                reason: "test".to_string(),
                detail: serde_json::json!({}),
            },
            1_000,
        )
        .unwrap();

    assert!(recovery
        .rollback_causes(&monitor, &root, "my-subject")
        .unwrap()
        .is_empty());
}

#[test]
fn a_rollback_cause_is_really_remembered_and_can_really_be_queried_back() {
    let (monitor, root, recovery, graph) = setup_with_memory();
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
        .recovery_point_create(&monitor, &root, Trigger::PreUpdate, &[node], 1_000)
        .unwrap();

    recovery
        .restore_to_with_cause(
            &monitor,
            &root,
            rp,
            "device-firmware",
            RollbackCause {
                reason: "rollout health breach at stage 1".to_string(),
                detail: serde_json::json!({"error_rate": 0.42, "threshold": 0.1}),
            },
            1_500,
        )
        .unwrap();

    let history = recovery
        .rollback_causes(&monitor, &root, "device-firmware")
        .unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].recovery_point_id, rp);
    assert_eq!(history[0].subject, "device-firmware");
    assert_eq!(history[0].cause.reason, "rollout health breach at stage 1");
    assert_eq!(
        history[0]
            .cause
            .detail
            .get("error_rate")
            .and_then(|v| v.as_f64()),
        Some(0.42)
    );
    assert_eq!(history[0].created_at, 1_500);
}

#[test]
fn rollback_causes_is_scoped_to_its_own_subject_and_returns_oldest_first() {
    let (monitor, root, recovery, graph) = setup_with_memory();
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

    for (subject, ts) in [
        ("subject-a", 1_000),
        ("subject-b", 1_100),
        ("subject-a", 1_200),
    ] {
        let rp = recovery
            .recovery_point_create(&monitor, &root, Trigger::PreUpdate, &[node], ts)
            .unwrap();
        recovery
            .restore_to_with_cause(
                &monitor,
                &root,
                rp,
                subject,
                RollbackCause {
                    reason: format!("attempt at {ts}"),
                    detail: serde_json::json!({}),
                },
                ts,
            )
            .unwrap();
    }

    let history_a = recovery
        .rollback_causes(&monitor, &root, "subject-a")
        .unwrap();
    assert_eq!(history_a.len(), 2);
    assert_eq!(history_a[0].created_at, 1_000, "oldest first");
    assert_eq!(history_a[1].created_at, 1_200);

    let history_b = recovery
        .rollback_causes(&monitor, &root, "subject-b")
        .unwrap();
    assert_eq!(history_b.len(), 1);

    assert!(recovery
        .rollback_causes(&monitor, &root, "no-such-subject")
        .unwrap()
        .is_empty());
}
