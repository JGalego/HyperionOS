//! docs/34 §2's ungated telemetry path (metrics/spans/logs) and §5's
//! k-anonymity aggregate gate.

use hyperion_observability::{
    build_aggregate, derivative, ewma, ConsentCategory, ConsentScope, LogEvent, LogLevel,
    MetricSample, RedactionClass, SpanStatus, TelemetryCollector,
};

#[test]
fn a_trace_reconstructs_every_span_recorded_under_it() {
    let collector = TelemetryCollector::new();
    let trace_id = 42;
    let root_span = collector.start_span(trace_id, "handle_intent", None, 1_000);
    let child_span = collector.start_span(trace_id, "web.research", Some(root_span), 1_001);
    collector.end_span(child_span, 1_050, SpanStatus::Ok);
    collector.end_span(root_span, 1_060, SpanStatus::Ok);

    let spans = collector.spans_for_trace(trace_id);
    assert_eq!(spans.len(), 2);
    assert!(spans
        .iter()
        .any(|s| s.span_id == child_span && s.parent_span_id == Some(root_span)));
}

#[test]
fn merge_remote_trace_reconstructs_the_whole_cross_device_trace_tree() {
    // Two independent collectors, standing in for two real federation
    // devices -- each only ever sees its own local slice of one shared
    // trace_id, the way hyperion-federation's per-device instances
    // genuinely would.
    let device_a = TelemetryCollector::new();
    let device_b = TelemetryCollector::new();
    let trace_id = 7;

    let root_span = device_a.start_span(trace_id, "handle_intent", None, 1_000);
    device_a.end_span(root_span, 1_010, SpanStatus::Ok);

    let remote_span = device_b.start_span(trace_id, "offloaded_capability", None, 1_020);
    device_b.end_span(remote_span, 1_090, SpanStatus::Ok);
    device_b.emit_log(LogEvent {
        level: LogLevel::Info,
        message: "ran on device B".to_string(),
        redaction_class: RedactionClass::None,
        timestamp: 1_050,
        trace_id: Some(trace_id),
    });

    assert_eq!(
        device_a.spans_for_trace(trace_id).len(),
        1,
        "before merging, device A only sees its own local span"
    );

    device_a.merge_remote_trace(trace_id, &device_b);

    let merged_spans = device_a.spans_for_trace(trace_id);
    assert_eq!(
        merged_spans.len(),
        2,
        "after merging, device A reconstructs the whole cross-device trace"
    );
    assert!(merged_spans.iter().any(|s| s.span_id == remote_span));
    let merged_logs = device_a.logs_for_trace(trace_id);
    assert_eq!(merged_logs.len(), 1);
    assert_eq!(merged_logs[0].message, "ran on device B");

    // Device B's own view is untouched -- merging is a one-way pull into
    // the receiving collector, not a two-way sync.
    assert_eq!(device_b.spans_for_trace(trace_id).len(), 1);
}

#[test]
fn two_collectors_built_with_distinct_device_ids_never_mint_a_colliding_span_id() {
    let device_a = TelemetryCollector::new_with_device_id(1);
    let device_b = TelemetryCollector::new_with_device_id(2);
    let trace_id = 99;

    // Both collectors mint their first-ever span at the same local sequence number -- only a
    // real per-device namespace (not just an independent per-process counter) can keep these
    // from colliding.
    let span_a = device_a.start_span(trace_id, "on_device_a", None, 1_000);
    let span_b = device_b.start_span(trace_id, "on_device_b", None, 1_000);
    assert_ne!(
        span_a, span_b,
        "two collectors with distinct real device_ids must never mint the same SpanId"
    );
    assert_eq!(span_a.device_id, 1);
    assert_eq!(span_b.device_id, 2);

    device_a.merge_remote_trace(trace_id, &device_b);
    let merged = device_a.spans_for_trace(trace_id);
    assert_eq!(merged.len(), 2);
    let ids: std::collections::HashSet<_> = merged.iter().map(|s| s.span_id).collect();
    assert_eq!(
        ids.len(),
        2,
        "merging two distinct devices' spans must never collapse them into a shared identity"
    );
}

#[test]
fn a_collector_built_via_new_defaults_to_device_id_zero() {
    let collector = TelemetryCollector::new();
    let span = collector.start_span(1, "root", None, 1_000);
    assert_eq!(span.device_id, 0);
}

#[test]
fn metrics_are_queryable_by_name() {
    let collector = TelemetryCollector::new();
    collector.record_metric(MetricSample {
        name: "cpu.util".to_string(),
        value: 0.4,
        unit: "ratio".to_string(),
        timestamp: 1_000,
        tags: Default::default(),
    });
    collector.record_metric(MetricSample {
        name: "battery.mw".to_string(),
        value: 500.0,
        unit: "mw".to_string(),
        timestamp: 1_000,
        tags: Default::default(),
    });

    assert_eq!(collector.metrics_named("cpu.util").len(), 1);
    assert_eq!(collector.metrics_named("battery.mw").len(), 1);
}

#[test]
fn logs_are_scoped_to_their_trace() {
    let collector = TelemetryCollector::new();
    collector.emit_log(LogEvent {
        level: LogLevel::Info,
        message: "started".to_string(),
        redaction_class: RedactionClass::None,
        timestamp: 1_000,
        trace_id: Some(1),
    });
    collector.emit_log(LogEvent {
        level: LogLevel::Warn,
        message: "unrelated".to_string(),
        redaction_class: RedactionClass::None,
        timestamp: 1_001,
        trace_id: Some(2),
    });

    assert_eq!(collector.logs_for_trace(1).len(), 1);
}

#[test]
fn ewma_smooths_toward_the_new_sample() {
    let smoothed = ewma(0.2, 0.8, 0.5);
    assert!((smoothed - 0.5).abs() < 1e-9);
}

#[test]
fn derivative_is_zero_for_a_non_positive_time_delta() {
    assert_eq!(derivative(10.0, 5.0, 0.0), 0.0);
}

#[test]
fn a_cohort_below_the_floor_is_suppressed_entirely_not_partially() {
    let scope = ConsentScope {
        category: ConsentCategory::PerfHealth,
        aggregation_min_cohort: 100,
    };
    let report = build_aggregate(&scope, 50, vec![("p50_latency_ms".to_string(), 120.0)]);
    assert!(report.suppressed);
    assert!(report.summaries.is_empty());
}

#[test]
fn no_consent_category_suppresses_even_a_large_cohort() {
    let scope = ConsentScope {
        category: ConsentCategory::None,
        aggregation_min_cohort: 10,
    };
    let report = build_aggregate(&scope, 10_000, vec![("p50_latency_ms".to_string(), 120.0)]);
    assert!(report.suppressed);
}

#[test]
fn an_opted_in_sufficiently_large_cohort_is_reported() {
    let scope = ConsentScope {
        category: ConsentCategory::CrashDiagnostics,
        aggregation_min_cohort: 50,
    };
    let report = build_aggregate(&scope, 100, vec![("crash_rate".to_string(), 0.001)]);
    assert!(!report.suppressed);
    assert_eq!(report.summaries.len(), 1);
}
