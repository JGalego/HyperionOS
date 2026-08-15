//! An interruptible "sleep until the next tick, unless we're told to stop first" for this crate's
//! own caller-paced background threads.
//!
//! The pattern this replaces -- `thread::sleep(interval)`, then re-check an `AtomicBool` -- only
//! notices a stop request when the sleep happens to end, so stopping costs up to a full interval.
//! That is invisible for this crate's fixed 20-50ms accept/retry loops (they poll far faster than
//! anyone waits on them) but not for [`crate::FederationHub::start_lease_heartbeat`], whose
//! interval is chosen by the caller and, for a real lease, measured in minutes. Blocking a
//! `Drop` for minutes is the kind of hang nobody debugs quickly, because nobody suspects the
//! `drop`.
//!
//! Parking on a [`Condvar`] a stop request notifies makes shutdown latency depend on the work in
//! flight rather than on the polling interval.
//!
//! Deliberately not shared with `hyperion-observability`'s identical primitive (which guards its
//! own audit-ledger verification schedule, and hit the same bug): this is ~20 lines of pure `std`
//! with no domain content, and neither crate is a plausible home for the other's threading
//! helpers. Inventing a dependency edge between an observability crate and a distributed-execution
//! crate to share it would couple two layers for less code than the coupling costs to explain.

use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// A one-way stop flag a waiting thread can be woken from immediately.
#[derive(Default)]
pub(crate) struct StopSignal {
    stopped: Mutex<bool>,
    woken: Condvar,
}

impl StopSignal {
    /// Waits up to `interval`, returning `true` if a stop was requested -- either already pending
    /// before the call, or signalled while waiting -- and `false` if the interval genuinely
    /// elapsed and the caller should run another tick.
    pub(crate) fn sleep_unless_stopped(&self, interval: Duration) -> bool {
        let stopped = self.stopped.lock().unwrap();
        if *stopped {
            return true;
        }
        let (stopped, _timeout) = self.woken.wait_timeout(stopped, interval).unwrap();
        *stopped
    }

    /// Requests a stop and wakes every waiter immediately. Idempotent.
    pub(crate) fn request_stop(&self) {
        *self.stopped.lock().unwrap() = true;
        self.woken.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Instant;

    #[test]
    fn a_pending_stop_is_seen_without_waiting_out_the_interval() {
        let signal = StopSignal::default();
        signal.request_stop();

        let started = Instant::now();
        assert!(signal.sleep_unless_stopped(Duration::from_secs(3_600)));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_stop_signalled_mid_wait_wakes_the_waiter_immediately() {
        let signal = Arc::new(StopSignal::default());
        let waiter = Arc::clone(&signal);
        let handle = std::thread::spawn(move || {
            let started = Instant::now();
            let stopped = waiter.sleep_unless_stopped(Duration::from_secs(3_600));
            (stopped, started.elapsed())
        });

        signal.request_stop();
        let (stopped, elapsed) = handle.join().unwrap();
        assert!(stopped, "the waiter must report the stop, not a timeout");
        assert!(
            elapsed < Duration::from_secs(5),
            "the waiter must be woken by the stop, not wait out the interval -- took {elapsed:?}"
        );
    }

    #[test]
    fn an_elapsed_interval_with_no_stop_reports_another_tick_is_due() {
        let signal = StopSignal::default();
        assert!(!signal.sleep_unless_stopped(Duration::from_millis(1)));
    }
}
