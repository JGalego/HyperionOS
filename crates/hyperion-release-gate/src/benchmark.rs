use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use crate::types::{
    BenchmarkBaseline, BenchmarkResult, GateAction, GateOutcome, HardwareProfileId, RegressionGate,
    RegressionThreshold,
};

/// Below this many real prior results in the window, there's nothing real to compute a
/// variance against — matches [`BenchmarkRegistry::record_result`]'s own "no baseline yet ->
/// `Pass`, not a fabricated verdict" reasoning for [`RegressionThreshold::Percent`].
const MIN_SAMPLES_FOR_SIGNIFICANCE: usize = 2;

/// docs/36 §2's `evaluate_gate` for [`RegressionThreshold::Percent`]: percent delta of `result`
/// against `baseline`, then the gate's configured `action` decides the outcome only if the delta
/// actually breaches the threshold.
pub fn evaluate_gate(
    gate: &RegressionGate,
    result_p99_ms: u32,
    baseline: &BenchmarkBaseline,
) -> (f32, GateOutcome) {
    let RegressionThreshold::Percent(threshold_pct) = gate.threshold else {
        panic!("evaluate_gate is the Percent-threshold path; Sigma gates go through evaluate_sigma_gate");
    };
    let delta_pct = if baseline.p99_ms == 0 {
        0.0
    } else {
        ((result_p99_ms as f32 - baseline.p99_ms as f32) / baseline.p99_ms as f32) * 100.0
    };

    if delta_pct <= threshold_pct {
        return (delta_pct, GateOutcome::Pass);
    }
    (delta_pct, outcome_for(gate.action))
}

/// docs/36 §1/§2's real statistical-significance test: a real z-score of `result_p99_ms` against
/// `history`'s own real, computed mean and (population) standard deviation, gated on
/// `sigma_threshold` real standard deviations. Fewer than [`MIN_SAMPLES_FOR_SIGNIFICANCE`] prior
/// results is nothing real to compute a variance against yet — `Pass`, not a fabricated verdict,
/// matching the `Percent` path's own "no baseline yet" reasoning. A real, exactly-zero variance
/// (every prior result identical) with a result that genuinely differs is still a real
/// regression signal, not a division-by-zero crash: any deviation at all from a real, perfectly
/// consistent history counts as maximally significant.
pub fn evaluate_sigma_gate(
    gate: &RegressionGate,
    result_p99_ms: u32,
    history: &[u32],
) -> (f32, GateOutcome) {
    let RegressionThreshold::Sigma(sigma_threshold) = gate.threshold else {
        panic!("evaluate_sigma_gate is the Sigma-threshold path; Percent gates go through evaluate_gate");
    };
    if history.len() < MIN_SAMPLES_FOR_SIGNIFICANCE {
        return (0.0, GateOutcome::Pass);
    }

    let n = history.len() as f32;
    let mean = history.iter().map(|&v| v as f32).sum::<f32>() / n;
    let variance = history
        .iter()
        .map(|&v| (v as f32 - mean).powi(2))
        .sum::<f32>()
        / n;
    let stddev = variance.sqrt();

    let z_score = if stddev > 0.0 {
        (result_p99_ms as f32 - mean) / stddev
    } else if (result_p99_ms as f32 - mean).abs() < f32::EPSILON {
        0.0
    } else {
        f32::MAX
    };

    if z_score.abs() <= sigma_threshold {
        return (z_score, GateOutcome::Pass);
    }
    (z_score, outcome_for(gate.action))
}

fn outcome_for(action: GateAction) -> GateOutcome {
    match action {
        GateAction::BlockRelease => GateOutcome::Blocked,
        GateAction::Warn => GateOutcome::Warned,
        GateAction::QuarantineAndRerun => GateOutcome::Quarantined,
    }
}

/// docs/36 §2's `gate_check`: registers baselines and gates keyed by
/// `(spec_id, hardware_profile)` — the doc's own key invariant, "never
/// cross-tier compare," is structural here: there is no lookup path that
/// takes a result from one hardware profile and compares it against
/// another's baseline.
pub struct BenchmarkRegistry {
    baselines: Mutex<HashMap<(String, HardwareProfileId), BenchmarkBaseline>>,
    gates: Mutex<HashMap<String, RegressionGate>>,
    /// docs/36 §1's `baseline_window` — real, trailing `p99_ms` history per `(spec_id,
    /// hardware_profile)`, consumed only by [`RegressionThreshold::Sigma`] gates
    /// ([`RegressionThreshold::Percent`] compares against `baselines` instead). Bounded to each
    /// gate's own `baseline_window_builds` so this never grows unboundedly across a long-running
    /// release-gate process.
    windows: Mutex<HashMap<(String, HardwareProfileId), VecDeque<u32>>>,
}

impl Default for BenchmarkRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl BenchmarkRegistry {
    pub fn new() -> Self {
        BenchmarkRegistry {
            baselines: Mutex::new(HashMap::new()),
            gates: Mutex::new(HashMap::new()),
            windows: Mutex::new(HashMap::new()),
        }
    }

    pub fn set_baseline(&self, spec_id: &str, hardware_profile: &str, baseline: BenchmarkBaseline) {
        self.baselines.lock().unwrap().insert(
            (spec_id.to_string(), hardware_profile.to_string()),
            baseline,
        );
    }

    pub fn set_gate(&self, spec_id: &str, gate: RegressionGate) {
        self.gates.lock().unwrap().insert(spec_id.to_string(), gate);
    }

    /// Records a result and evaluates it against the same-tier baseline (a single point for
    /// [`RegressionThreshold::Percent`], a real rolling window's mean/stddev for
    /// [`RegressionThreshold::Sigma`]). A spec/profile pair with no baseline/history yet has
    /// nothing to regress against — `Pass`, not a fabricated verdict.
    pub fn record_result(&self, result: &BenchmarkResult) -> (f32, GateOutcome) {
        let gate = self
            .gates
            .lock()
            .unwrap()
            .get(&result.spec_id)
            .copied()
            .unwrap_or(RegressionGate {
                threshold: RegressionThreshold::Percent(10.0),
                action: GateAction::Warn,
                baseline_window_builds: 0,
            });

        match gate.threshold {
            RegressionThreshold::Percent(_) => {
                let baseline = self
                    .baselines
                    .lock()
                    .unwrap()
                    .get(&(result.spec_id.clone(), result.hardware_profile.clone()))
                    .copied();
                let Some(baseline) = baseline else {
                    return (0.0, GateOutcome::Pass);
                };
                evaluate_gate(&gate, result.p99_ms, &baseline)
            }
            RegressionThreshold::Sigma(_) => {
                let key = (result.spec_id.clone(), result.hardware_profile.clone());
                let mut windows = self.windows.lock().unwrap();
                let history: Vec<u32> = windows.get(&key).cloned().unwrap_or_default().into();
                let outcome = evaluate_sigma_gate(&gate, result.p99_ms, &history);

                let window = windows.entry(key).or_default();
                window.push_back(result.p99_ms);
                let cap = gate.baseline_window_builds.max(1) as usize;
                while window.len() > cap {
                    window.pop_front();
                }
                outcome
            }
        }
    }
}
