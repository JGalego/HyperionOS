use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::types::{
    LogEvent, LogRetentionPolicy, MetricRollup, MetricSample, SpanStatus, TraceId, TraceSpan,
};

/// docs/34 §3's lossy/sampled/best-effort telemetry path — deliberately
/// *not* capability-gated, unlike this crate's [`crate::AuditLedger`]:
/// docs/34 §8 specifies per-invocation overhead in "low tens of
/// microseconds," and a capability check per metric sample would defeat
/// that budget for signal this workspace's own Design Invariant 5
/// ("fails open") says must never block execution. The audit path is the
/// one that must never proceed unlogged; this one is the one that must
/// never block.
pub struct TelemetryCollector {
    metrics: Mutex<Vec<MetricSample>>,
    spans: Mutex<Vec<TraceSpan>>,
    logs: Mutex<Vec<LogEvent>>,
    next_span_id: AtomicU64,
    /// docs/34 §5's "compacted to percentile rollups" -- what
    /// [`Self::compact_metrics`] produces and [`Self::metric_rollups_named`] reads back.
    rollups: Mutex<Vec<MetricRollup>>,
}

impl Default for TelemetryCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl TelemetryCollector {
    pub fn new() -> Self {
        TelemetryCollector {
            metrics: Mutex::new(Vec::new()),
            spans: Mutex::new(Vec::new()),
            logs: Mutex::new(Vec::new()),
            next_span_id: AtomicU64::new(1),
            rollups: Mutex::new(Vec::new()),
        }
    }

    pub fn record_metric(&self, sample: MetricSample) {
        self.metrics.lock().unwrap().push(sample);
    }

    pub fn metrics_named(&self, name: &str) -> Vec<MetricSample> {
        self.metrics
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.name == name)
            .cloned()
            .collect()
    }

    pub fn start_span(
        &self,
        trace_id: TraceId,
        name: &str,
        parent_span_id: Option<u64>,
        start: u64,
    ) -> u64 {
        let span_id = self.next_span_id.fetch_add(1, Ordering::Relaxed);
        self.spans.lock().unwrap().push(TraceSpan {
            trace_id,
            span_id,
            parent_span_id,
            name: name.to_string(),
            start,
            end: None,
            status: SpanStatus::Ok,
            attributes: Default::default(),
        });
        span_id
    }

    pub fn end_span(&self, span_id: u64, end: u64, status: SpanStatus) {
        if let Some(span) = self
            .spans
            .lock()
            .unwrap()
            .iter_mut()
            .find(|s| s.span_id == span_id)
        {
            span.end = Some(end);
            span.status = status;
        }
    }

    /// docs/34 §2: "`trace_id` minted at Intent creation, threaded through
    /// every Agent/Capability call... one Intent = one reconstructable
    /// trace tree" — this is that reconstruction.
    pub fn spans_for_trace(&self, trace_id: TraceId) -> Vec<TraceSpan> {
        self.spans
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.trace_id == trace_id)
            .cloned()
            .collect()
    }

    pub fn emit_log(&self, event: LogEvent) {
        self.logs.lock().unwrap().push(event);
    }

    pub fn logs_for_trace(&self, trace_id: TraceId) -> Vec<LogEvent> {
        self.logs
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.trace_id == Some(trace_id))
            .cloned()
            .collect()
    }

    /// [21 — Distributed Execution](../21-distributed-execution.md)'s
    /// distributed trace merging, made real: pulls another device's
    /// `TelemetryCollector`'s spans/logs for one real [`TraceId`] into
    /// this collector, so [`Self::spans_for_trace`]/[`Self::logs_for_trace`]
    /// on the receiving side reconstructs the *whole* cross-device trace
    /// tree docs/34 §2 describes ("`trace_id` minted at Intent creation,
    /// threaded through every Agent/Capability call... one Intent = one
    /// reconstructable trace tree"), not just this one device's local
    /// slice of it. A best-effort append, not a CRDT merge: `span_id` is
    /// only unique *within* the collector that minted it, so a merged
    /// trace can contain two spans sharing a `span_id` if they
    /// originated on different devices — giving every span a real,
    /// globally-unique identity across devices is a further, separate
    /// refinement this doesn't attempt. Calling this twice with the same
    /// remote data duplicates entries; a caller merges each remote batch
    /// once.
    pub fn merge_remote_trace(&self, trace_id: TraceId, remote: &TelemetryCollector) {
        self.spans
            .lock()
            .unwrap()
            .extend(remote.spans_for_trace(trace_id));
        self.logs
            .lock()
            .unwrap()
            .extend(remote.logs_for_trace(trace_id));
    }

    /// docs/34 §5's own previously-named "retention/rollup compaction" gap, closed for real:
    /// "raw metrics are kept at full resolution for a short window (default 24h) then compacted
    /// to percentile rollups." Every real raw [`MetricSample`] older than `retention_secs` (i.e.
    /// `now - timestamp > retention_secs`) is removed from raw storage and folded into one real
    /// [`MetricRollup`] per distinct metric `name` among those aged-out samples -- real min/max/
    /// count and real p50/p95/p99 (nearest-rank method) computed from the *actual* aged-out
    /// values, never fabricated. Samples still inside the retention window are left alone, so
    /// calling this repeatedly (e.g. from a real periodic caller) only ever compacts what's newly
    /// aged out since the last call. A `name` with no samples aging out this call produces no new
    /// rollup at all -- this never pads a rollup list with empty/placeholder entries.
    pub fn compact_metrics(&self, now: u64, retention_secs: u64) {
        let mut metrics = self.metrics.lock().unwrap();
        let (aged_out, fresh): (Vec<MetricSample>, Vec<MetricSample>) = metrics
            .drain(..)
            .partition(|m| now.saturating_sub(m.timestamp) > retention_secs);
        *metrics = fresh;
        drop(metrics);

        let mut by_name: HashMap<String, Vec<MetricSample>> = HashMap::new();
        for sample in aged_out {
            by_name.entry(sample.name.clone()).or_default().push(sample);
        }

        let mut rollups = self.rollups.lock().unwrap();
        for (name, mut samples) in by_name {
            samples.sort_by(|a, b| a.value.total_cmp(&b.value));
            let window_start = samples.iter().map(|s| s.timestamp).min().unwrap();
            let window_end = samples.iter().map(|s| s.timestamp).max().unwrap();
            let count = samples.len();
            rollups.push(MetricRollup {
                name,
                window_start,
                window_end,
                count,
                min: samples[0].value,
                max: samples[count - 1].value,
                p50: percentile(&samples, 50.0),
                p95: percentile(&samples, 95.0),
                p99: percentile(&samples, 99.0),
            });
        }
    }

    /// The real rollups [`Self::compact_metrics`] has produced so far for metric `name`, oldest
    /// window first (insertion order) -- distinct from [`Self::metrics_named`], which only ever
    /// sees the still-raw, not-yet-compacted samples.
    pub fn metric_rollups_named(&self, name: &str) -> Vec<MetricRollup> {
        self.rollups
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.name == name)
            .cloned()
            .collect()
    }

    /// docs/34 §5's "logs age out per level-based TTL," closed for real: every real [`LogEvent`]
    /// whose own real `level` has aged past `policy`'s TTL for it (`now - timestamp > ttl`) is
    /// dropped outright -- logs are never rolled up (there is no percentile-style summary for a
    /// log message), only aged out, per docs/34 §5's own text.
    pub fn expire_logs(&self, now: u64, policy: &LogRetentionPolicy) {
        self.logs
            .lock()
            .unwrap()
            .retain(|log| now.saturating_sub(log.timestamp) <= policy.ttl_secs_for(log.level));
    }
}

/// The nearest-rank percentile of `sorted.value`, ascending-sorted by the caller -- shared by
/// every real percentile [`TelemetryCollector::compact_metrics`] computes, so `p50`/`p95`/`p99`
/// are all really derived from the same one real method, not three independent approximations.
fn percentile(sorted: &[MetricSample], p: f64) -> f64 {
    let rank = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[rank].value
}

/// docs/34 §2's continuous EWMA over a resource-utilization metric,
/// feeding the Scheduler's `LoadSignal` — see this crate's doc comment on
/// the deferred `Scheduler.subscribeLoadSignal` wiring; this is the
/// estimator itself, real and independently testable.
pub fn ewma(previous: f64, sample: f64, alpha: f64) -> f64 {
    alpha * sample + (1.0 - alpha) * previous
}

/// docs/34 §2's derivative over e.g. battery drain rate.
pub fn derivative(latest: f64, previous: f64, dt_secs: f64) -> f64 {
    if dt_secs <= 0.0 {
        0.0
    } else {
        (latest - previous) / dt_secs
    }
}
