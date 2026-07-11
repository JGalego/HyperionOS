//! Real `SCHED_DEADLINE` for the `RealTimeUI` class — chosen over `SCHED_RR` (the roadmap's
//! other listed option) because it is the more faithful mapping, not because it's more
//! sophisticated: docs/04-scheduler.md's `RealTimeUI` class is already dispatched by
//! Earliest-Deadline-First in `hyperion_scheduler`'s own algorithm, and `SCHED_DEADLINE` is a
//! real, in-kernel EDF implementation. Applying it doesn't approximate the algorithm, it *is*
//! the algorithm, enforced by the kernel instead of an in-memory sort.
//!
//! `sched_setattr(2)` has no glibc/libc wrapper (it's newer than the classic
//! `sched_setscheduler(2)` API and takes a variable-sized, versioned struct `sched_setscheduler`
//! has no room for), so this calls the syscall directly via `libc::syscall`, with the
//! `sched_attr` struct defined here to match the kernel's stable ABI
//! (`include/uapi/linux/sched/types.h`).

use std::io;

use crate::RealtimeError;

/// Mirrors the kernel's `struct sched_attr`. Field order and sizes are ABI, not a Rust
/// convention — this must match `include/uapi/linux/sched/types.h` exactly for the syscall to
/// interpret it correctly instead of reading garbage past whatever fields differ.
#[repr(C)]
struct SchedAttr {
    size: u32,
    sched_policy: u32,
    sched_flags: u64,
    sched_nice: i32,
    sched_priority: u32,
    sched_runtime: u64,
    sched_deadline: u64,
    sched_period: u64,
}

/// A real-time budget for one `RealTimeUI` task, in the same units
/// `sched_setattr(2)` expects: nanoseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeadlineBudget {
    /// Worst-case execution time this task is guaranteed within each period.
    pub runtime_ns: u64,
    /// The absolute deadline within the period by which `runtime_ns` must have run.
    pub deadline_ns: u64,
    /// How often the runtime budget replenishes.
    pub period_ns: u64,
}

/// Applies real `SCHED_DEADLINE` scheduling to `pid` (0 meaning the calling thread, per
/// `sched_setattr(2)`'s own convention) with `budget`. Requires `CAP_SYS_NICE` (or root) on
/// every Linux system, sandboxed or not — this is a kernel admission-control policy protecting
/// real-time bandwidth system-wide, not a Hyperion-specific restriction to work around. Without
/// it this fails with `EPERM`, which this function surfaces as
/// [`RealtimeError::SchedSetattr`] rather than silently downgrading to a normal priority: a
/// caller that needs to know whether its `RealTimeUI` task actually got real deadline scheduling
/// must not be told it succeeded when it didn't.
pub fn apply_sched_deadline(pid: libc::pid_t, budget: DeadlineBudget) -> Result<(), RealtimeError> {
    let attr = SchedAttr {
        size: std::mem::size_of::<SchedAttr>() as u32,
        sched_policy: libc::SCHED_DEADLINE as u32,
        sched_flags: 0,
        sched_nice: 0,
        sched_priority: 0,
        sched_runtime: budget.runtime_ns,
        sched_deadline: budget.deadline_ns,
        sched_period: budget.period_ns,
    };

    // SAFETY: `attr` is a valid, correctly-sized `sched_attr` for the duration of this call;
    // `flags` is 0 (no special behavior requested), matching every documented `sched_setattr(2)`
    // example.
    let rc = unsafe {
        libc::syscall(
            libc::SYS_sched_setattr,
            pid,
            &attr as *const SchedAttr,
            0u32,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(RealtimeError::SchedSetattr(io::Error::last_os_error()))
    }
}

/// Fallback real-time policy via the classic `sched_setscheduler(2)` API (which *does* have a
/// libc wrapper) — the roadmap's other listed option, for a kernel or use case where
/// `SCHED_DEADLINE`'s admission control is a worse fit than a plain fixed-priority round-robin
/// class. Not used by [`crate::apply`] by default; kept available for exactly the case its own
/// module docs describe.
pub fn apply_sched_rr(pid: libc::pid_t, priority: i32) -> Result<(), RealtimeError> {
    let param = libc::sched_param {
        sched_priority: priority,
    };
    // SAFETY: `param` is a valid `sched_param` for the duration of this call.
    let rc = unsafe { libc::sched_setscheduler(pid, libc::SCHED_RR, &param) };
    if rc == 0 {
        Ok(())
    } else {
        Err(RealtimeError::SchedSetscheduler(io::Error::last_os_error()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This sandbox has no `CAP_SYS_NICE` and no `rtprio` rlimit budget (verified directly:
    /// `ulimit -r` is 0), so real `SCHED_DEADLINE` cannot be *granted* here — that's a
    /// system/kernel privilege boundary this crate correctly respects, not a bug in it. What
    /// *is* verifiable in this sandbox: the syscall is well-formed and really reaches the
    /// kernel's admission control, which is what makes the failure `EPERM` specifically rather
    /// than `EINVAL` (a malformed request) or some other error. A future session with real root
    /// or `CAP_SYS_NICE` (e.g. inside the booted image once M5 exists) should see this succeed
    /// instead.
    #[test]
    fn sched_deadline_reaches_real_kernel_admission_control() {
        let result = apply_sched_deadline(
            0,
            DeadlineBudget {
                runtime_ns: 10_000_000,
                deadline_ns: 100_000_000,
                period_ns: 100_000_000,
            },
        );
        match result {
            Ok(()) => {
                // A future, privileged environment: real success is the actual best outcome.
            }
            Err(RealtimeError::SchedSetattr(e)) => {
                assert_eq!(
                    e.raw_os_error(),
                    Some(libc::EPERM),
                    "expected EPERM (no CAP_SYS_NICE in this sandbox), got a different errno \
                     -- that would indicate a malformed syscall, a real bug, not a privilege gap: {e}"
                );
            }
            Err(other) => panic!("unexpected error variant: {other}"),
        }
    }
}
