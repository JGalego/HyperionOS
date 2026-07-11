//! Mirrors every other crate in this workspace: every call is capability-
//! gated, re-checked live against the monitor. `SystemImageController`
//! is deliberately not capability-gated at all — see
//! `hyperion_update`'s crate doc comment on why the bootloader-level
//! track sits below this layer.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_recovery::RecoveryService;
use hyperion_update::{
    sign, CohortHealth, HealthThresholds, RolloutPolicy, UpdateError, UpdateManifest,
    UpdateOrchestrator, UpdateSubject,
};

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

fn manifest(keystore: &Keystore) -> UpdateManifest {
    let mut m = UpdateManifest {
        subject: UpdateSubject::Capability {
            id: "document.draft".to_string(),
        },
        from_version: 0,
        to_version: 1,
        signature: None,
        touched_objects: vec![],
        rollout_policy: RolloutPolicy::default_schedule(HealthThresholds {
            max_crash_rate: 0.1,
            max_latency_p99_ms: 1_000,
        }),
    };
    m.signature = Some(sign(&m, keystore));
    m
}

fn orchestrator() -> UpdateOrchestrator {
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    UpdateOrchestrator::new(Arc::new(RecoveryService::new(graph)))
}

#[test]
fn apply_update_requires_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();
    let orchestrator = orchestrator();
    let (_dir, keystore) = keystore();

    let result = orchestrator.apply_update(
        &monitor,
        &read_only,
        &manifest(&keystore),
        true,
        1_000,
        |_| CohortHealth {
            crash_rate: 0.0,
            latency_p99_ms: 50,
        },
        &keystore.verifying_key(),
    );
    assert!(matches!(result, Err(UpdateError::Unauthorized)));
}

#[test]
fn update_rollback_requires_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let orchestrator = orchestrator();
    let (_dir, keystore) = keystore();
    let m = manifest(&keystore);
    orchestrator
        .apply_update(
            &monitor,
            &root,
            &m,
            true,
            1_000,
            |_| CohortHealth {
                crash_rate: 0.0,
                latency_p99_ms: 50,
            },
            &keystore.verifying_key(),
        )
        .unwrap();

    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();
    let result = orchestrator.update_rollback(&monitor, &read_only, &m);
    assert!(matches!(result, Err(UpdateError::Unauthorized)));
}

#[test]
fn revoking_the_token_blocks_further_access_re_checked_live() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let delegate = monitor
        .cap_derive(&root, RightsMask::all(), None, TrustBoundaryId(2))
        .unwrap();
    let orchestrator = orchestrator();
    let (_dir, keystore) = keystore();

    assert!(orchestrator
        .apply_update(
            &monitor,
            &delegate,
            &manifest(&keystore),
            true,
            1_000,
            |_| CohortHealth {
                crash_rate: 0.0,
                latency_p99_ms: 50,
            },
            &keystore.verifying_key(),
        )
        .is_ok());

    monitor.cap_revoke(&delegate);

    let mut second = manifest(&keystore);
    second.from_version = 1;
    second.to_version = 2;
    second.signature = Some(sign(&second, &keystore));
    assert!(matches!(
        orchestrator.apply_update(
            &monitor,
            &delegate,
            &second,
            true,
            1_001,
            |_| CohortHealth {
                crash_rate: 0.0,
                latency_p99_ms: 50,
            },
            &keystore.verifying_key(),
        ),
        Err(UpdateError::Unauthorized)
    ));
}
