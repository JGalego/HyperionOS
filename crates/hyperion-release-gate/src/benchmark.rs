use std::collections::HashMap;
use std::sync::Mutex;

use crate::types::{
    BenchmarkBaseline, BenchmarkResult, GateAction, GateOutcome, HardwareProfileId, RegressionGate,
};

/// docs/36 §2's `evaluate_gate`: percent delta of `result` against
/// `baseline`, then the gate's configured `action` decides the outcome
/// only if the delta actually breaches `threshold_pct`.
pub fn evaluate_gate(
    gate: &RegressionGate,
    result_p99_ms: u32,
    baseline: &BenchmarkBaseline,
) -> (f32, GateOutcome) {
    let delta_pct = if baseline.p99_ms == 0 {
        0.0
    } else {
        ((result_p99_ms as f32 - baseline.p99_ms as f32) / baseline.p99_ms as f32) * 100.0
    };

    if delta_pct <= gate.threshold_pct {
        return (delta_pct, GateOutcome::Pass);
    }
    let outcome = match gate.action {
        GateAction::BlockRelease => GateOutcome::Blocked,
        GateAction::Warn => GateOutcome::Warned,
        GateAction::QuarantineAndRerun => GateOutcome::Quarantined,
    };
    (delta_pct, outcome)
}

/// docs/36 §2's `gate_check`: registers baselines and gates keyed by
/// `(spec_id, hardware_profile)` — the doc's own key invariant, "never
/// cross-tier compare," is structural here: there is no lookup path that
/// takes a result from one hardware profile and compares it against
/// another's baseline.
pub struct BenchmarkRegistry {
    baselines: Mutex<HashMap<(String, HardwareProfileId), BenchmarkBaseline>>,
    gates: Mutex<HashMap<String, RegressionGate>>,
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

    /// Records a result and evaluates it against the same-tier baseline,
    /// if one exists. A spec/profile pair with no baseline yet has
    /// nothing to regress against — `Pass`, not a fabricated verdict.
    pub fn record_result(&self, result: &BenchmarkResult) -> (f32, GateOutcome) {
        let baseline = self
            .baselines
            .lock()
            .unwrap()
            .get(&(result.spec_id.clone(), result.hardware_profile.clone()))
            .copied();
        let Some(baseline) = baseline else {
            return (0.0, GateOutcome::Pass);
        };
        let gate = self
            .gates
            .lock()
            .unwrap()
            .get(&result.spec_id)
            .copied()
            .unwrap_or(RegressionGate {
                threshold_pct: 10.0,
                action: GateAction::Warn,
            });
        evaluate_gate(&gate, result.p99_ms, &baseline)
    }
}
