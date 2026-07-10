//! Mirrors every other crate in this workspace: every call is capability-
//! gated, re-checked live against the monitor. `SystemImageController`
//! is deliberately not capability-gated at all — see
//! `hyperion_update`'s crate doc comment on why the bootloader-level
//! track sits below this layer.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_recovery::RecoveryService;
use hyperion_update::{
    signature, CohortHealth, HealthThresholds, RolloutPolicy, UpdateError, UpdateManifest,
    UpdateOrchestrator, UpdateSubject,
};

fn manifest() -> UpdateManifest {
    let mut m = UpdateManifest {
        subject: UpdateSubject::Capability {
            id: "document.draft".to_string(),
        },
        from_version: 0,
        to_version: 1,
        signature: 0,
        touched_objects: vec![],
        rollout_policy: RolloutPolicy::default_schedule(HealthThresholds {
            max_crash_rate: 0.1,
            max_latency_p99_ms: 1_000,
        }),
    };
    m.signature = signature(&m);
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

    let result = orchestrator.apply_update(&monitor, &read_only, &manifest(), true, 1_000, |_| {
        CohortHealth {
            crash_rate: 0.0,
            latency_p99_ms: 50,
        }
    });
    assert!(matches!(result, Err(UpdateError::Unauthorized)));
}

#[test]
fn update_rollback_requires_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let orchestrator = orchestrator();
    let m = manifest();
    orchestrator
        .apply_update(&monitor, &root, &m, true, 1_000, |_| CohortHealth {
            crash_rate: 0.0,
            latency_p99_ms: 50,
        })
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

    assert!(orchestrator
        .apply_update(&monitor, &delegate, &manifest(), true, 1_000, |_| {
            CohortHealth {
                crash_rate: 0.0,
                latency_p99_ms: 50,
            }
        })
        .is_ok());

    monitor.cap_revoke(&delegate);

    let mut second = manifest();
    second.from_version = 1;
    second.to_version = 2;
    second.signature = signature(&second);
    assert!(matches!(
        orchestrator.apply_update(&monitor, &delegate, &second, true, 1_001, |_| {
            CohortHealth {
                crash_rate: 0.0,
                latency_p99_ms: 50,
            }
        }),
        Err(UpdateError::Unauthorized)
    ));
}
