//! docs/36 §1/§2's real statistical-significance test: `RegressionThreshold::Sigma` gates on a
//! real z-score against a real, computed rolling-window mean/standard deviation, not a flat
//! percentage -- "a single noisy run cannot block a release" (a real result within its own
//! history's real variance passes), and "a real, small, consistent regression cannot hide inside
//! noise" (a result genuinely outside that variance is flagged).

use hyperion_release_gate::{
    BenchmarkRegistry, BenchmarkResult, GateAction, GateOutcome, RegressionGate,
    RegressionThreshold,
};

fn result(p99_ms: u32) -> BenchmarkResult {
    BenchmarkResult {
        spec_id: "boot.cold".to_string(),
        hardware_profile: "laptop".to_string(),
        p50_ms: p99_ms.saturating_sub(50),
        p95_ms: p99_ms.saturating_sub(10),
        p99_ms,
    }
}

fn sigma_gate(sigma: f32, action: GateAction, window_builds: u32) -> RegressionGate {
    RegressionGate {
        threshold: RegressionThreshold::Sigma(sigma),
        action,
        baseline_window_builds: window_builds,
    }
}

#[test]
fn fewer_than_two_prior_results_has_nothing_to_regress_against() {
    let registry = BenchmarkRegistry::new();
    registry.set_gate("boot.cold", sigma_gate(2.0, GateAction::BlockRelease, 10));

    let (delta, outcome) = registry.record_result(&result(5_000));
    assert_eq!(delta, 0.0);
    assert_eq!(outcome, GateOutcome::Pass);

    // Only one real prior result now exists -- still not enough for a real variance.
    let (delta, outcome) = registry.record_result(&result(5_010));
    assert_eq!(delta, 0.0);
    assert_eq!(outcome, GateOutcome::Pass);
}

#[test]
fn a_result_within_the_real_historical_variance_passes() {
    let registry = BenchmarkRegistry::new();
    registry.set_gate("boot.cold", sigma_gate(2.0, GateAction::BlockRelease, 10));

    // The first two real results establish a history with mean 4950ms, stddev 50ms; a third
    // result at 5030ms is only 1.6 real standard deviations away -- ordinary variance, not a
    // regression.
    registry.record_result(&result(4_900));
    registry.record_result(&result(5_000));
    let (_, outcome) = registry.record_result(&result(5_030));
    assert_eq!(
        outcome,
        GateOutcome::Pass,
        "ordinary variance within this real history must never block a release"
    );
}

#[test]
fn a_result_far_outside_the_real_historical_variance_blocks() {
    let registry = BenchmarkRegistry::new();
    registry.set_gate("boot.cold", sigma_gate(2.0, GateAction::BlockRelease, 10));

    // A real, tight, consistent history around 5000ms...
    registry.record_result(&result(4_990));
    registry.record_result(&result(5_000));
    registry.record_result(&result(5_010));
    // ...then a real, genuine regression far outside that variance.
    let (z_score, outcome) = registry.record_result(&result(6_000));
    assert!(
        z_score.abs() > 2.0,
        "expected a real z-score past 2 sigma, got {z_score}"
    );
    assert_eq!(outcome, GateOutcome::Blocked);
}

#[test]
fn the_same_breach_only_warns_when_the_gate_action_is_warn() {
    let registry = BenchmarkRegistry::new();
    registry.set_gate("boot.cold", sigma_gate(2.0, GateAction::Warn, 10));

    registry.record_result(&result(4_990));
    registry.record_result(&result(5_000));
    registry.record_result(&result(5_010));
    let (_, outcome) = registry.record_result(&result(6_000));
    assert_eq!(outcome, GateOutcome::Warned);
}

#[test]
fn an_exactly_identical_history_with_a_genuinely_different_result_is_flagged_not_a_crash() {
    let registry = BenchmarkRegistry::new();
    registry.set_gate("boot.cold", sigma_gate(2.0, GateAction::BlockRelease, 10));

    // A real, perfectly consistent history -- zero real variance.
    registry.record_result(&result(5_000));
    registry.record_result(&result(5_000));
    // Any real deviation at all from a perfectly consistent history is maximally significant.
    let (_, outcome) = registry.record_result(&result(5_001));
    assert_eq!(outcome, GateOutcome::Blocked);
}

#[test]
fn an_exactly_identical_history_with_an_identical_result_still_passes() {
    let registry = BenchmarkRegistry::new();
    registry.set_gate("boot.cold", sigma_gate(2.0, GateAction::BlockRelease, 10));

    registry.record_result(&result(5_000));
    registry.record_result(&result(5_000));
    let (z_score, outcome) = registry.record_result(&result(5_000));
    assert_eq!(z_score, 0.0);
    assert_eq!(outcome, GateOutcome::Pass);
}

#[test]
fn the_rolling_window_is_bounded_and_drops_old_results() {
    let registry = BenchmarkRegistry::new();
    // A tiny window: only the 2 most recent real results are ever kept.
    registry.set_gate("boot.cold", sigma_gate(2.0, GateAction::BlockRelease, 2));

    // These two real, wildly different early results would normally make the history's own
    // variance huge -- but the window is capped at 2, so by the time we probe a real result,
    // only the most recent 2 of *those* count, not these two.
    registry.record_result(&result(1_000));
    registry.record_result(&result(9_000));
    // These two overwrite the window with a real, tight, consistent pair.
    registry.record_result(&result(5_000));
    registry.record_result(&result(5_000));

    // A result far from 1_000/9_000's own huge real variance, but consistent with the real,
    // tight window that's now actually in effect, must still be flagged as a real regression --
    // proving the old, evicted results no longer influence the score at all.
    let (_, outcome) = registry.record_result(&result(5_500));
    assert_eq!(outcome, GateOutcome::Blocked);
}

#[test]
fn different_hardware_tiers_never_share_a_rolling_window() {
    let registry = BenchmarkRegistry::new();
    registry.set_gate("boot.cold", sigma_gate(2.0, GateAction::BlockRelease, 10));

    registry.record_result(&result(4_990));
    registry.record_result(&result(5_000));
    registry.record_result(&result(5_010));

    let sbc_result = BenchmarkResult {
        spec_id: "boot.cold".to_string(),
        hardware_profile: "sbc".to_string(),
        p50_ms: 8_000,
        p95_ms: 9_500,
        p99_ms: 9_900,
    };
    // The sbc tier has never been measured under this gate -- it must not silently borrow the
    // laptop tier's real rolling window.
    let (delta, outcome) = registry.record_result(&sbc_result);
    assert_eq!(delta, 0.0);
    assert_eq!(outcome, GateOutcome::Pass);
}
