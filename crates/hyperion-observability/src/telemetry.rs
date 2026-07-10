use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::types::{LogEvent, MetricSample, SpanStatus, TraceId, TraceSpan};

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
