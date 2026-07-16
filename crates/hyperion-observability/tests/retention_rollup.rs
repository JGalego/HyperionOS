//! docs/34 §5's own previously-named "retention/rollup compaction" gap: raw metrics kept at full
//! resolution for a short window then compacted to real percentile rollups; logs age out per a
//! real level-based TTL.

use std::collections::HashMap;

use hyperion_observability::{
    LogEvent, LogLevel, LogRetentionPolicy, MetricSample, RedactionClass, TelemetryCollector,
};

fn sample(name: &str, value: f64, timestamp: u64) -> MetricSample {
    MetricSample {
        name: name.to_string(),
        value,
        unit: "ms".to_string(),
        timestamp,
        tags: HashMap::new(),
    }
}

#[test]
fn a_fresh_sample_inside_the_retention_window_is_left_raw() {
    let collector = TelemetryCollector::new();
    collector.record_metric(sample("latency", 10.0, 1_000));

    // now - timestamp = 100, well inside a 3600s retention window.
    collector.compact_metrics(1_100, 3_600);

    assert_eq!(collector.metrics_named("latency").len(), 1);
    assert!(collector.metric_rollups_named("latency").is_empty());
}

#[test]
fn an_aged_out_sample_is_removed_from_raw_storage_and_folded_into_a_real_rollup() {
    let collector = TelemetryCollector::new();
    // now - timestamp = 4000, past a 3600s retention window.
    collector.record_metric(sample("latency", 10.0, 1_000));

    collector.compact_metrics(5_000, 3_600);

    assert!(
        collector.metrics_named("latency").is_empty(),
        "an aged-out sample must be removed from raw storage"
    );
    let rollups = collector.metric_rollups_named("latency");
    assert_eq!(rollups.len(), 1);
    assert_eq!(rollups[0].count, 1);
    assert_eq!(rollups[0].min, 10.0);
    assert_eq!(rollups[0].max, 10.0);
    assert_eq!(rollups[0].p50, 10.0);
}

#[test]
fn a_rollups_percentiles_are_really_computed_from_the_aged_out_values() {
    let collector = TelemetryCollector::new();
    // Ten samples, 1 through 10, all aged out together.
    for v in 1..=10 {
        collector.record_metric(sample("latency", v as f64, 1_000));
    }
    collector.compact_metrics(5_000, 3_600);

    let rollups = collector.metric_rollups_named("latency");
    assert_eq!(rollups.len(), 1);
    let r = &rollups[0];
    assert_eq!(r.count, 10);
    assert_eq!(r.min, 1.0);
    assert_eq!(r.max, 10.0);
    // Nearest-rank on a sorted [1..10]: p50 index = round(0.5*9) = 5 -> value 6.0 (0-indexed).
    assert_eq!(r.p50, 6.0);
    assert_eq!(r.p99, 10.0);
}

#[test]
fn two_different_metric_names_among_aged_out_samples_get_two_separate_rollups() {
    let collector = TelemetryCollector::new();
    collector.record_metric(sample("latency", 10.0, 1_000));
    collector.record_metric(sample("cpu.util", 55.0, 1_000));

    collector.compact_metrics(5_000, 3_600);

    assert_eq!(collector.metric_rollups_named("latency").len(), 1);
    assert_eq!(collector.metric_rollups_named("cpu.util").len(), 1);
    assert_eq!(collector.metric_rollups_named("cpu.util")[0].min, 55.0);
}

#[test]
fn compacting_again_with_nothing_newly_aged_out_adds_no_empty_rollup() {
    let collector = TelemetryCollector::new();
    collector.record_metric(sample("latency", 10.0, 1_000));
    collector.compact_metrics(5_000, 3_600);
    assert_eq!(collector.metric_rollups_named("latency").len(), 1);

    // A second compaction pass with no new raw samples aged out must not add a second,
    // empty/placeholder rollup.
    collector.compact_metrics(5_100, 3_600);
    assert_eq!(collector.metric_rollups_named("latency").len(), 1);
}

#[test]
fn a_second_batch_of_aged_out_samples_produces_a_second_real_rollup_window() {
    let collector = TelemetryCollector::new();
    collector.record_metric(sample("latency", 10.0, 1_000));
    collector.compact_metrics(5_000, 3_600);

    collector.record_metric(sample("latency", 20.0, 6_000));
    collector.compact_metrics(10_000, 3_600);

    let rollups = collector.metric_rollups_named("latency");
    assert_eq!(
        rollups.len(),
        2,
        "each newly aged-out batch gets its own real rollup"
    );
    assert_eq!(rollups[1].min, 20.0);
}

fn log(level: LogLevel, timestamp: u64) -> LogEvent {
    LogEvent {
        level,
        message: "test".to_string(),
        redaction_class: RedactionClass::None,
        timestamp,
        trace_id: None,
    }
}

#[test]
fn an_expired_log_is_really_gone_a_fresh_one_survives() {
    let collector = TelemetryCollector::new();
    let policy = LogRetentionPolicy {
        trace_ttl_secs: 10,
        debug_ttl_secs: 10,
        info_ttl_secs: 100,
        warn_ttl_secs: 1_000,
        error_ttl_secs: 10_000,
    };

    let mut expired = log(LogLevel::Trace, 1_000);
    expired.trace_id = Some(1);
    let mut survives = log(LogLevel::Error, 1_000);
    survives.trace_id = Some(1);

    collector.emit_log(expired);
    collector.emit_log(survives);

    collector.expire_logs(1_050, &policy);

    let remaining = collector.logs_for_trace(1);
    assert_eq!(
        remaining.len(),
        1,
        "only the real, still-within-TTL log must survive"
    );
    assert_eq!(remaining[0].level, LogLevel::Error);
}
