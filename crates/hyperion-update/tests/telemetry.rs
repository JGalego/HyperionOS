//! Proves `cohort_health_from_telemetry` reads a real
//! `hyperion_observability::TelemetryCollector`'s real metric samples,
//! and that the resulting `CohortHealth` drives a real staged rollout
//! exactly as a caller-invented one would.

use std::collections::HashMap;
use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_observability::{MetricSample, TelemetryCollector};
use hyperion_recovery::RecoveryService;
use hyperion_update::{
    cohort_health_from_telemetry, signature, HealthThresholds, RolloutPolicy, RolloutState,
    UpdateError, UpdateManifest, UpdateOrchestrator, UpdateSubject,
};

fn record(telemetry: &TelemetryCollector, name: &str, value: f64, timestamp: u64) {
    telemetry.record_metric(MetricSample {
        name: name.to_string(),
        value,
        unit: String::new(),
        timestamp,
        tags: HashMap::new(),
    });
}

#[test]
fn cohort_health_from_telemetry_is_none_until_both_metrics_have_a_sample() {
    let telemetry = TelemetryCollector::new();
    assert!(cohort_health_from_telemetry(&telemetry, "crash_rate", "latency_p99_ms").is_none());

    record(&telemetry, "crash_rate", 0.0, 1_000);
    assert!(
        cohort_health_from_telemetry(&telemetry, "crash_rate", "latency_p99_ms").is_none(),
        "latency_p99_ms still has no sample"
    );

    record(&telemetry, "latency_p99_ms", 80.0, 1_000);
    assert!(cohort_health_from_telemetry(&telemetry, "crash_rate", "latency_p99_ms").is_some());
}

#[test]
fn cohort_health_from_telemetry_reads_the_most_recent_sample() {
    let telemetry = TelemetryCollector::new();
    record(&telemetry, "crash_rate", 0.5, 1_000);
    record(&telemetry, "crash_rate", 0.0, 2_000); // more recent -- this one should win
    record(&telemetry, "latency_p99_ms", 900.0, 1_000);
    record(&telemetry, "latency_p99_ms", 40.0, 2_000);

    let health = cohort_health_from_telemetry(&telemetry, "crash_rate", "latency_p99_ms").unwrap();
    assert_eq!(health.crash_rate, 0.0);
    assert_eq!(health.latency_p99_ms, 40);
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
        rollout_policy: RolloutPolicy::default_schedule(HealthThresholds {
            max_crash_rate: 0.01,
            max_latency_p99_ms: 500,
        }),
    };
    m.signature = signature(&m);
    m
}

#[test]
fn a_real_telemetry_backed_health_reading_drives_the_rollout_to_completion() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let recovery = Arc::new(RecoveryService::new(graph));
    let orchestrator = UpdateOrchestrator::new(recovery);
    let telemetry = TelemetryCollector::new();
    record(&telemetry, "crash_rate", 0.0, 1_000);
    record(&telemetry, "latency_p99_ms", 50.0, 1_000);

    let m = manifest(vec![]);
    let version = orchestrator
        .apply_update(&monitor, &root, &m, true, 1_000, |_percent| {
            cohort_health_from_telemetry(&telemetry, "crash_rate", "latency_p99_ms")
                .expect("both metrics were recorded before the rollout started")
        })
        .unwrap();

    assert_eq!(version, 1);
    assert_eq!(
        orchestrator.rollout_state(&m.subject),
        Some(RolloutState::RolledOut)
    );
}

#[test]
fn a_real_telemetry_backed_crash_spike_triggers_automatic_rollback() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let recovery = Arc::new(RecoveryService::new(graph));
    let orchestrator = UpdateOrchestrator::new(recovery);
    let telemetry = TelemetryCollector::new();
    record(&telemetry, "crash_rate", 0.5, 1_000); // breaches max_crash_rate: 0.01
    record(&telemetry, "latency_p99_ms", 50.0, 1_000);

    let m = manifest(vec![]);
    let result = orchestrator.apply_update(&monitor, &root, &m, true, 1_000, |_percent| {
        cohort_health_from_telemetry(&telemetry, "crash_rate", "latency_p99_ms")
            .expect("both metrics were recorded before the rollout started")
    });

    assert!(matches!(result, Err(UpdateError::RolloutHealthBreach)));
    assert_eq!(
        orchestrator.rollout_state(&m.subject),
        Some(RolloutState::RolledBack)
    );
}
