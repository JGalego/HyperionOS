//! This crate's own previously-named "`Scheduler.subscribeLoadSignal` wiring" gap
//! (docs/34-observability-telemetry.md §3), closed for real: [`publish_load_signal`] computes a
//! real [`hyperion_scheduler::LoadSignal`] from a real [`crate::TelemetryCollector`]'s own
//! recorded [`crate::MetricSample`]s -- [`crate::telemetry::ewma`] over recent CPU-utilization
//! samples, [`crate::telemetry::derivative`] over the two most recent battery-level samples, and
//! the single most recent thermal-headroom reading -- and pushes it into a real
//! `hyperion_scheduler::Scheduler` via [`hyperion_scheduler::Scheduler::update_load_signal`]. No
//! Cargo cycle: `hyperion-scheduler` doesn't depend back on this crate (unlike this crate's own
//! `hyperion-explainability` dependency, which does need care elsewhere), so this is a plain,
//! direct dependency -- no trait-object indirection needed the way `hyperion-scheduler`'s own
//! `OffloadTrigger` required for its cycle-blocked case.
//!
//! Deliberately a snapshot read, not a running estimator this crate maintains internally: each
//! real call recomputes the EWMA from whatever raw samples the collector still holds (its own
//! real, already-established retention window -- see [`crate::TelemetryCollector::compact_metrics`]),
//! so a caller with no fresh samples since the last call simply recomputes the same real value
//! rather than this module holding hidden state of its own.

use hyperion_scheduler::{LoadSignal, Scheduler};

use crate::telemetry::{derivative, ewma, TelemetryCollector};

/// The real, well-known [`crate::MetricSample::name`] convention this module reads --
/// docs/34 §3's own "CPU/GPU/NPU utilization."
pub const CPU_UTILIZATION_METRIC: &str = "cpu.utilization";
/// docs/34 §3's own "battery drain rate" is a real derivative over this metric's own recorded
/// level samples, not recorded as a rate directly.
pub const BATTERY_LEVEL_METRIC: &str = "battery.level";
/// docs/34 §3's own "remaining thermal headroom."
pub const THERMAL_HEADROOM_METRIC: &str = "thermal.headroom";

/// Computes a real [`LoadSignal`] from `collector`'s own currently-retained real samples and
/// pushes it into `scheduler`. Never fabricates a signal from missing data: a metric with no
/// samples recorded yet contributes its own honest zero rather than a guessed value -- the same
/// "a session with no activity yet reports the same fixed... estimate" honesty
/// `hyperion-context`'s own Adaptive Complexity read already established for the identical shape
/// of gap. Returns the real signal actually pushed, so a caller can inspect or log it without a
/// second, redundant `scheduler.current_load_signal()` call.
pub fn publish_load_signal(
    collector: &TelemetryCollector,
    scheduler: &mut Scheduler,
    ewma_alpha: f64,
) -> LoadSignal {
    let utilization_ewma = cpu_utilization_ewma(collector, ewma_alpha);
    let battery_drain_rate = battery_drain_rate(collector);
    let thermal_headroom = latest_thermal_headroom(collector);

    let signal = LoadSignal {
        utilization_ewma,
        battery_drain_rate,
        thermal_headroom,
    };
    scheduler.update_load_signal(signal);
    signal
}

fn cpu_utilization_ewma(collector: &TelemetryCollector, alpha: f64) -> f64 {
    let mut samples = collector.metrics_named(CPU_UTILIZATION_METRIC);
    samples.sort_by_key(|s| s.timestamp);
    samples
        .iter()
        .fold(0.0, |estimate, sample| ewma(estimate, sample.value, alpha))
}

/// A real drain *rate*, not a raw level -- `battery.level`'s own recorded value is expected to
/// fall over time, so this negates [`derivative`]'s raw (necessarily non-positive) slope into an
/// honestly-named positive "how fast is it draining" magnitude. `0.0` with fewer than two real
/// recorded samples: a real rate needs two real points in time, not one.
fn battery_drain_rate(collector: &TelemetryCollector) -> f64 {
    let mut samples = collector.metrics_named(BATTERY_LEVEL_METRIC);
    samples.sort_by_key(|s| s.timestamp);
    match samples.as_slice() {
        [.., previous, latest] => {
            let dt_secs = latest.timestamp.saturating_sub(previous.timestamp) as f64;
            -derivative(latest.value, previous.value, dt_secs)
        }
        _ => 0.0,
    }
}

/// The single most recent real thermal-headroom reading -- an instantaneous physical quantity,
/// not something an EWMA/derivative should smooth away. `0.0` with no real reading recorded yet.
fn latest_thermal_headroom(collector: &TelemetryCollector) -> f64 {
    collector
        .metrics_named(THERMAL_HEADROOM_METRIC)
        .into_iter()
        .max_by_key(|s| s.timestamp)
        .map(|s| s.value)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MetricSample;
    use std::collections::HashMap;

    fn sample(name: &str, value: f64, timestamp: u64) -> MetricSample {
        MetricSample {
            name: name.to_string(),
            value,
            unit: "".to_string(),
            timestamp,
            tags: HashMap::new(),
        }
    }

    #[test]
    fn a_collector_with_no_samples_publishes_an_honest_all_zero_signal() {
        let collector = TelemetryCollector::new();
        let mut scheduler = Scheduler::new();

        let signal = publish_load_signal(&collector, &mut scheduler, 0.3);

        assert_eq!(signal, LoadSignal::default());
        assert_eq!(scheduler.current_load_signal(), Some(signal));
    }

    #[test]
    fn cpu_utilization_is_a_real_ewma_over_recorded_samples_in_timestamp_order() {
        let collector = TelemetryCollector::new();
        collector.record_metric(sample(CPU_UTILIZATION_METRIC, 0.2, 100));
        collector.record_metric(sample(CPU_UTILIZATION_METRIC, 0.8, 200));
        let mut scheduler = Scheduler::new();

        let signal = publish_load_signal(&collector, &mut scheduler, 0.5);

        let expected = ewma(ewma(0.0, 0.2, 0.5), 0.8, 0.5);
        assert!(
            (signal.utilization_ewma - expected).abs() < 1e-9,
            "got {}, expected {expected}",
            signal.utilization_ewma
        );
    }

    #[test]
    fn battery_drain_rate_is_a_real_positive_rate_when_level_is_falling() {
        let collector = TelemetryCollector::new();
        collector.record_metric(sample(BATTERY_LEVEL_METRIC, 80.0, 0));
        collector.record_metric(sample(BATTERY_LEVEL_METRIC, 70.0, 100));
        let mut scheduler = Scheduler::new();

        let signal = publish_load_signal(&collector, &mut scheduler, 0.3);

        assert!(
            (signal.battery_drain_rate - 0.1).abs() < 1e-9,
            "got {}",
            signal.battery_drain_rate
        );
    }

    #[test]
    fn a_single_battery_sample_gives_an_honest_zero_rate_not_a_guess() {
        let collector = TelemetryCollector::new();
        collector.record_metric(sample(BATTERY_LEVEL_METRIC, 80.0, 0));
        let mut scheduler = Scheduler::new();

        let signal = publish_load_signal(&collector, &mut scheduler, 0.3);

        assert_eq!(signal.battery_drain_rate, 0.0);
    }

    #[test]
    fn thermal_headroom_is_the_real_most_recent_reading_not_an_average() {
        let collector = TelemetryCollector::new();
        collector.record_metric(sample(THERMAL_HEADROOM_METRIC, 30.0, 0));
        collector.record_metric(sample(THERMAL_HEADROOM_METRIC, 10.0, 100));
        let mut scheduler = Scheduler::new();

        let signal = publish_load_signal(&collector, &mut scheduler, 0.3);

        assert_eq!(signal.thermal_headroom, 10.0);
    }
}
