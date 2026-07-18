//! docs/34-observability-telemetry.md §3's own `Scheduler.subscribeLoadSignal` -- this crate's
//! own previously-named "no subscription API to receive one" gap. See
//! `hyperion-observability::scheduler_feedback` for the real production caller that computes a
//! real `LoadSignal` from real telemetry and pushes it in.

use hyperion_scheduler::{LoadSignal, Scheduler};

#[test]
fn a_fresh_scheduler_has_no_load_signal_yet() {
    let scheduler = Scheduler::new();
    assert_eq!(scheduler.current_load_signal(), None);
}

#[test]
fn a_pushed_load_signal_is_read_back_exactly() {
    let mut scheduler = Scheduler::new();
    let signal = LoadSignal {
        utilization_ewma: 0.62,
        battery_drain_rate: 1.5,
        thermal_headroom: 12.0,
    };

    scheduler.update_load_signal(signal);

    assert_eq!(scheduler.current_load_signal(), Some(signal));
}

#[test]
fn pushing_a_new_signal_replaces_the_previous_one() {
    let mut scheduler = Scheduler::new();
    scheduler.update_load_signal(LoadSignal {
        utilization_ewma: 0.2,
        battery_drain_rate: 0.5,
        thermal_headroom: 20.0,
    });
    let second = LoadSignal {
        utilization_ewma: 0.9,
        battery_drain_rate: 3.0,
        thermal_headroom: 4.0,
    };
    scheduler.update_load_signal(second);

    assert_eq!(scheduler.current_load_signal(), Some(second));
}
