use hyperion_observability::TelemetryCollector;

use crate::types::CohortHealth;

/// docs/34-shaped `observability::cohort_health`, made real: reads the
/// most recent real `hyperion_observability::TelemetryCollector` samples
/// named `crash_rate_metric`/`latency_p99_metric` and builds a
/// [`CohortHealth`] from them, instead of a caller inventing the numbers
/// from nothing. Returns `None` when either metric has no sample
/// recorded yet for this stage — this crate does not decide what "no
/// reading" should mean for a rollout decision (fail-open vs
/// fail-closed); the caller's own `health_for_stage` closure passed to
/// [`crate::orchestrator::UpdateOrchestrator::apply_update`] does,
/// exactly as it already decides what [`CohortHealth`] to pass today.
pub fn cohort_health_from_telemetry(
    telemetry: &TelemetryCollector,
    crash_rate_metric: &str,
    latency_p99_metric: &str,
) -> Option<CohortHealth> {
    Some(CohortHealth {
        crash_rate: latest_value(telemetry, crash_rate_metric)? as f32,
        latency_p99_ms: latest_value(telemetry, latency_p99_metric)? as u32,
    })
}

fn latest_value(telemetry: &TelemetryCollector, name: &str) -> Option<f64> {
    telemetry
        .metrics_named(name)
        .into_iter()
        .max_by_key(|sample| sample.timestamp)
        .map(|sample| sample.value)
}
