//! docs/32 §1's staged, health-gated rollout: monotonic advancement
//! through every stage on healthy signal, and automatic rollback — via
//! the real `hyperion-recovery` crate — on a health breach.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_recovery::RecoveryService;
use hyperion_update::{
    signature, CohortHealth, HealthThresholds, RolloutPolicy, RolloutState, UpdateError,
    UpdateManifest, UpdateOrchestrator, UpdateSubject,
};

fn setup() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    UpdateOrchestrator,
    Arc<KnowledgeGraph>,
) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let recovery = Arc::new(RecoveryService::new(graph.clone()));
    let orchestrator = UpdateOrchestrator::new(recovery);
    (monitor, root, orchestrator, graph)
}

fn healthy() -> HealthThresholds {
    HealthThresholds {
        max_crash_rate: 0.01,
        max_latency_p99_ms: 500,
    }
}

fn manifest(touched: Vec<hyperion_storage::ObjectId>) -> UpdateManifest {
    let mut m = UpdateManifest {
        subject: UpdateSubject::Capability {
            id: "document.draft".to_string(),
        },
        from_version: 0,
        to_version: 1,
        signature: 0,
        touched_objects: touched,
        rollout_policy: RolloutPolicy::default_schedule(healthy()),
    };
    m.signature = signature(&m);
    m
}

#[test]
fn a_healthy_rollout_advances_through_every_stage_and_commits() {
    let (monitor, root, orchestrator, _graph) = setup();
    let m = manifest(vec![]);

    let version = orchestrator
        .apply_update(&monitor, &root, &m, true, 1_000, |_percent| CohortHealth {
            crash_rate: 0.0,
            latency_p99_ms: 50,
        })
        .unwrap();

    assert_eq!(version, 1);
    assert_eq!(orchestrator.active_version(&m.subject), 1);
    assert_eq!(
        orchestrator.rollout_state(&m.subject),
        Some(RolloutState::RolledOut)
    );
}

#[test]
fn default_schedule_is_1_10_50_100() {
    let policy = RolloutPolicy::default_schedule(healthy());
    let percents: Vec<u8> = policy.stages.iter().map(|s| s.percent).collect();
    assert_eq!(percents, vec![1, 10, 50, 100]);
}

#[test]
fn a_health_breach_triggers_automatic_rollback_and_restores_touched_objects() {
    let (monitor, root, orchestrator, graph) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Config",
            None,
            serde_json::json!({"flag": "old"}),
        )
        .unwrap();
    let m = manifest(vec![node]);

    // The recovery point is taken before any stage runs, capturing
    // "old". This crate has no real Capability execution, so the first
    // (healthy) stage's health check is where the test simulates the
    // update's own effect landing, before the second stage breaches.
    let mut calls = 0;
    let result = orchestrator.apply_update(&monitor, &root, &m, true, 1_000, |_percent| {
        calls += 1;
        if calls == 1 {
            graph
                .put_node(
                    &monitor,
                    &root,
                    Some(node),
                    "Config",
                    None,
                    serde_json::json!({"flag": "new"}),
                )
                .unwrap();
            CohortHealth {
                crash_rate: 0.0,
                latency_p99_ms: 50,
            }
        } else {
            CohortHealth {
                crash_rate: 0.5,
                latency_p99_ms: 5_000,
            } // breach on the second stage
        }
    });

    assert!(matches!(result, Err(UpdateError::RolloutHealthBreach)));
    assert_eq!(
        orchestrator.active_version(&m.subject),
        0,
        "a breached rollout must never advance the active-version pointer"
    );
    assert_eq!(
        orchestrator.rollout_state(&m.subject),
        Some(RolloutState::RolledBack)
    );

    let restored = graph.get(&monitor, &root, node).unwrap();
    assert_eq!(restored.metadata["flag"], serde_json::json!("old"));
}

#[test]
fn a_health_breach_without_auto_rollback_leaves_data_untouched() {
    let (monitor, root, orchestrator, graph) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Config",
            None,
            serde_json::json!({"flag": "old"}),
        )
        .unwrap();
    let mut m = manifest(vec![node]);
    m.rollout_policy.auto_rollback_on_breach = false;
    m.signature = signature(&m);

    graph
        .put_node(
            &monitor,
            &root,
            Some(node),
            "Config",
            None,
            serde_json::json!({"flag": "new"}),
        )
        .unwrap();

    let result =
        orchestrator.apply_update(&monitor, &root, &m, true, 1_000, |_percent| CohortHealth {
            crash_rate: 1.0,
            latency_p99_ms: 9_999,
        });

    assert!(matches!(result, Err(UpdateError::RolloutHealthBreach)));
    let current = graph.get(&monitor, &root, node).unwrap();
    assert_eq!(
        current.metadata["flag"],
        serde_json::json!("new"),
        "without auto_rollback_on_breach, data must be left exactly as the failed rollout left it"
    );
}

#[test]
fn a_manifest_with_a_stale_from_version_is_incompatible() {
    let (monitor, root, orchestrator, _graph) = setup();
    let first = manifest(vec![]);
    orchestrator
        .apply_update(&monitor, &root, &first, true, 1_000, |_| CohortHealth {
            crash_rate: 0.0,
            latency_p99_ms: 50,
        })
        .unwrap();

    // A second manifest still claiming from_version=0, even though the
    // subject is already at version 1.
    let stale = manifest(vec![]);
    let result =
        orchestrator.apply_update(&monitor, &root, &stale, true, 1_010, |_| CohortHealth {
            crash_rate: 0.0,
            latency_p99_ms: 50,
        });
    assert!(matches!(result, Err(UpdateError::Incompatible)));
}

#[test]
fn a_hardware_incompatible_manifest_is_rejected_before_any_recovery_point_is_taken() {
    let (monitor, root, orchestrator, _graph) = setup();
    let m = manifest(vec![]);

    let result = orchestrator.apply_update(&monitor, &root, &m, false, 1_000, |_| CohortHealth {
        crash_rate: 0.0,
        latency_p99_ms: 50,
    });
    assert!(matches!(result, Err(UpdateError::Incompatible)));
    assert_eq!(
        orchestrator.rollout_state(&m.subject),
        None,
        "incompatibility must be caught before the pipeline ever stages or reaches Canary"
    );
}

#[test]
fn a_tampered_manifest_fails_signature_verification() {
    let (monitor, root, orchestrator, _graph) = setup();
    let mut m = manifest(vec![]);
    m.to_version = 2; // tampered after signing

    let result = orchestrator.apply_update(&monitor, &root, &m, true, 1_000, |_| CohortHealth {
        crash_rate: 0.0,
        latency_p99_ms: 50,
    });
    assert!(matches!(result, Err(UpdateError::SignatureInvalid)));
}

#[test]
fn a_post_hoc_rollback_after_a_full_rollout_restores_data_and_the_version_pointer() {
    let (monitor, root, orchestrator, graph) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Config",
            None,
            serde_json::json!({"flag": "old"}),
        )
        .unwrap();
    let m = manifest(vec![node]);

    orchestrator
        .apply_update(&monitor, &root, &m, true, 1_000, |_| CohortHealth {
            crash_rate: 0.0,
            latency_p99_ms: 50,
        })
        .unwrap();
    // The rolled-out version subsequently wrote something operators now
    // want to revert.
    graph
        .put_node(
            &monitor,
            &root,
            Some(node),
            "Config",
            None,
            serde_json::json!({"flag": "new-and-bad"}),
        )
        .unwrap();

    let receipt = orchestrator.update_rollback(&monitor, &root, &m).unwrap();
    assert_eq!(receipt.rolled_back_to, 0);
    assert_eq!(orchestrator.active_version(&m.subject), 0);

    let restored = graph.get(&monitor, &root, node).unwrap();
    assert_eq!(restored.metadata["flag"], serde_json::json!("old"));
}
