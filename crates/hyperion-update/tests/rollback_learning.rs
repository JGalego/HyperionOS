//! docs/998-roadmap.md's Self-Sustaining pillar: a rollback's real cause now really shapes a
//! future decision, not just a future log line -- retrying the exact same (subject,
//! from_version, to_version) that already rolled back once for a health breach is refused
//! outright, before the rollout even starts again; a genuinely different update for the same
//! subject is not blocked by that history.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_memory::MemoryEngine;
use hyperion_recovery::RecoveryService;
use hyperion_update::{
    sign, CohortHealth, HealthThresholds, RolloutPolicy, UpdateError, UpdateManifest,
    UpdateOrchestrator, UpdateSubject,
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
    let memory = Arc::new(MemoryEngine::new(graph.clone()));
    let recovery = Arc::new(RecoveryService::new_with_memory(
        graph.clone(),
        Some(memory),
    ));
    let orchestrator = UpdateOrchestrator::new(recovery);
    (monitor, root, orchestrator, graph)
}

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

fn strict_thresholds() -> HealthThresholds {
    HealthThresholds {
        max_crash_rate: 0.01,
        max_latency_p99_ms: 500,
    }
}

fn manifest(
    touched: Vec<hyperion_storage::ObjectId>,
    to_version: u32,
    keystore: &Keystore,
) -> UpdateManifest {
    let mut m = UpdateManifest {
        subject: UpdateSubject::Capability {
            id: "document.draft".to_string(),
        },
        from_version: 0,
        to_version,
        signature: None,
        touched_objects: touched,
        rollout_policy: RolloutPolicy::default_schedule(strict_thresholds()),
    };
    m.signature = Some(sign(&m, keystore));
    m
}

fn unhealthy() -> CohortHealth {
    CohortHealth {
        crash_rate: 0.5,
        latency_p99_ms: 9_999,
    }
}

#[test]
fn retrying_the_exact_same_update_after_a_health_breach_rollback_is_refused() {
    let (monitor, root, orchestrator, graph) = setup();
    let (_dir, keystore) = keystore();
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
    let m = manifest(vec![node], 1, &keystore);

    let first = orchestrator.apply_update(
        &monitor,
        &root,
        &m,
        true,
        1_000,
        |_percent| unhealthy(),
        &keystore.verifying_key(),
    );
    assert!(matches!(first, Err(UpdateError::RolloutHealthBreach)));

    let mut health_for_stage_called = false;
    let second = orchestrator.apply_update(
        &monitor,
        &root,
        &m,
        true,
        2_000,
        |_percent| {
            health_for_stage_called = true;
            unhealthy()
        },
        &keystore.verifying_key(),
    );
    assert!(
        matches!(second, Err(UpdateError::RepeatedRecentRollback { .. })),
        "got: {second:?}"
    );
    assert!(
        !health_for_stage_called,
        "a repeated update refused by history must never even start the rollout again"
    );
}

#[test]
fn a_genuinely_different_update_for_the_same_subject_is_not_blocked_by_unrelated_history() {
    let (monitor, root, orchestrator, graph) = setup();
    let (_dir, keystore) = keystore();
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
    let first_manifest = manifest(vec![node], 1, &keystore);

    let first = orchestrator.apply_update(
        &monitor,
        &root,
        &first_manifest,
        true,
        1_000,
        |_percent| unhealthy(),
        &keystore.verifying_key(),
    );
    assert!(matches!(first, Err(UpdateError::RolloutHealthBreach)));

    // A different to_version for the same subject -- not the update that rolled back.
    let second_manifest = manifest(vec![node], 2, &keystore);
    let second = orchestrator.apply_update(
        &monitor,
        &root,
        &second_manifest,
        true,
        2_000,
        |_percent| CohortHealth {
            crash_rate: 0.0,
            latency_p99_ms: 50,
        },
        &keystore.verifying_key(),
    );
    assert!(second.is_ok(), "got: {second:?}");
}
