//! docs/36 §2's `gate_check`: same-tier-only baseline comparison, no
//! regression until the threshold is breached, and the configured
//! action decides the outcome once it is.

use hyperion_release_gate::{
    BenchmarkBaseline, BenchmarkRegistry, BenchmarkResult, GateAction, GateOutcome, RegressionGate,
    RegressionThreshold,
};

fn result(p99_ms: u32) -> BenchmarkResult {
    BenchmarkResult {
        spec_id: "boot.cold".to_string(),
        hardware_profile: "laptop".to_string(),
        p50_ms: p99_ms - 50,
        p95_ms: p99_ms - 10,
        p99_ms,
    }
}

#[test]
fn no_baseline_yet_is_a_pass() {
    let registry = BenchmarkRegistry::new();
    let (delta, outcome) = registry.record_result(&result(5_000));
    assert_eq!(delta, 0.0);
    assert_eq!(outcome, GateOutcome::Pass);
}

#[test]
fn a_result_within_threshold_of_baseline_passes() {
    let registry = BenchmarkRegistry::new();
    registry.set_baseline("boot.cold", "laptop", BenchmarkBaseline { p99_ms: 4_500 });
    registry.set_gate(
        "boot.cold",
        RegressionGate {
            threshold: RegressionThreshold::Percent(5.0),
            action: GateAction::BlockRelease,
            baseline_window_builds: 0,
        },
    );

    let (_, outcome) = registry.record_result(&result(4_600)); // +2.2%
    assert_eq!(outcome, GateOutcome::Pass);
}

#[test]
fn a_result_beyond_threshold_blocks_when_the_gate_action_is_block_release() {
    let registry = BenchmarkRegistry::new();
    registry.set_baseline("boot.cold", "laptop", BenchmarkBaseline { p99_ms: 4_500 });
    registry.set_gate(
        "boot.cold",
        RegressionGate {
            threshold: RegressionThreshold::Percent(5.0),
            action: GateAction::BlockRelease,
            baseline_window_builds: 0,
        },
    );

    let (delta, outcome) = registry.record_result(&result(5_000)); // +11.1%
    assert!(delta > 5.0);
    assert_eq!(outcome, GateOutcome::Blocked);
}

#[test]
fn the_same_breach_only_warns_when_the_gate_action_is_warn() {
    let registry = BenchmarkRegistry::new();
    registry.set_baseline("boot.cold", "laptop", BenchmarkBaseline { p99_ms: 4_500 });
    registry.set_gate(
        "boot.cold",
        RegressionGate {
            threshold: RegressionThreshold::Percent(5.0),
            action: GateAction::Warn,
            baseline_window_builds: 0,
        },
    );

    let (_, outcome) = registry.record_result(&result(5_000));
    assert_eq!(outcome, GateOutcome::Warned);
}

#[test]
fn a_different_hardware_tier_never_compares_against_another_tiers_baseline() {
    let registry = BenchmarkRegistry::new();
    registry.set_baseline("boot.cold", "laptop", BenchmarkBaseline { p99_ms: 4_500 });
    registry.set_gate(
        "boot.cold",
        RegressionGate {
            threshold: RegressionThreshold::Percent(5.0),
            action: GateAction::BlockRelease,
            baseline_window_builds: 0,
        },
    );

    // No baseline registered for "sbc" — even though the laptop baseline
    // would flag this p99 as a huge regression, the sbc tier has never
    // been measured and must not silently borrow the laptop's baseline.
    let sbc_result = BenchmarkResult {
        spec_id: "boot.cold".to_string(),
        hardware_profile: "sbc".to_string(),
        p50_ms: 8_000,
        p95_ms: 9_500,
        p99_ms: 9_900,
    };
    let (delta, outcome) = registry.record_result(&sbc_result);
    assert_eq!(delta, 0.0);
    assert_eq!(outcome, GateOutcome::Pass);
}
