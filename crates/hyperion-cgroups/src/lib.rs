//! Real cgroups v2 + real scheduling policy enforcement for `hyperion-scheduler`'s admission and
//! fairness decisions, per
//! [PRODUCTION_BOOT_PROMPT.md](../../../PRODUCTION_BOOT_PROMPT.md) M4.
//!
//! Reuses `hyperion-scheduler`'s admission-control and DRF/EDF fairness math as-is — nothing
//! here recomputes *whether* or *how much* to admit. This crate only decides what real cgroup
//! v2 configuration and real scheduling policy *express* a decision the scheduler already made:
//! `cpu.weight` for the three DRF-shared classes (see [`mapping`]), and real `SCHED_DEADLINE`
//! for `RealTimeUI` (see [`realtime`]), which the kernel enforces independently of cgroup CPU
//! shares entirely (a real-time-scheduled thread preempts ordinary CFS-scheduled ones
//! regardless of `cpu.weight`, mirroring `RealTimeUI`'s reserved-headroom independence from the
//! DRF pool at the algorithm layer).
//!
//! ## Delegation, not root
//!
//! This crate never assumes it can create cgroups anywhere in `/sys/fs/cgroup` — it always
//! operates beneath a `parent` cgroup the caller already has write access to (already
//! *delegated*, in cgroup v2's own terminology). In this sandbox that's a systemd user session's
//! own delegated subtree (`/sys/fs/cgroup/user.slice/user-<uid>.slice/user@<uid>.service/...`,
//! verified writable by an unprivileged user directly); once M5's real root-owned `hyperion-init`
//! exists, it would instead pass its own dedicated subtree, created with real root at boot. The
//! mapping and enforcement logic in this crate doesn't change between those two cases — only
//! what `parent` path is passed in does.
//!
//! ## What's real here vs. deferred, and why
//!
//! - `cpu`, `memory`, `pids`: fully real and live-tested in this sandbox (see
//!   `tests/real_fairness.rs`) — real cgroups, real competing processes, real
//!   `cpu.stat` accounting.
//! - `io`: the `io.max` line format is implemented and unit-tested ([`mapping::io_max_line_for`]),
//!   but actually writing it needs a real block device's major:minor, which only exists once
//!   M6 gives this system real storage — and this sandbox's own delegation doesn't expose the
//!   `io` controller at all regardless (verified: not in `cgroup.controllers` here).
//! - `SCHED_DEADLINE`: implemented for real via the raw `sched_setattr(2)` syscall (see
//!   [`realtime`]), verified to reach real kernel admission control (rejected with `EPERM`,
//!   proving the syscall is well-formed, not that it's unreachable) — this sandbox has no
//!   `CAP_SYS_NICE` and no `rtprio` budget to actually be granted the policy, a kernel privilege
//!   boundary this crate correctly respects rather than working around.
//! - GPU controller: no GPU driver or cgroup controller exists to map onto in any environment
//!   this roadmap targets yet (docs/03's GPU scheduling class is a real, separate driver-model
//!   project — see M7's own display-stack scope note for the same class of deferral).

mod cgroup;
mod errors;
pub mod mapping;
pub mod realtime;

use std::path::Path;

pub use cgroup::Cgroup;
pub use errors::{CgroupError, RealtimeError};
use hyperion_scheduler::ResourceVector;

/// Creates a real cgroup named `name` beneath `parent` (which must already be a delegated,
/// writable cgroup directory) and applies `priority_weight`/`request`'s real `cpu.weight` and
/// `memory.max`/`pids.max` — the real-transport-equivalent of "admit this task," expressed as
/// real OS configuration instead of an in-memory ledger update. The scheduler's own
/// `Scheduler::schedule_epoch` still decides *whether* to admit; this is what runs *after* it
/// says yes.
pub fn enforce_admission(
    parent: impl AsRef<Path>,
    name: &str,
    priority_weight: f32,
    request: ResourceVector,
) -> Result<Cgroup, CgroupError> {
    let cgroup = Cgroup::create(parent, name)?;
    let config = mapping::config_for(priority_weight, request);

    cgroup.write_control_file("cpu.weight", &config.cpu_weight.to_string())?;
    if let Some(mem) = config.memory_max_bytes {
        cgroup.write_control_file("memory.max", &mem.to_string())?;
    }
    if let Some(pids) = config.pids_max {
        cgroup.write_control_file("pids.max", &pids.to_string())?;
    }

    Ok(cgroup)
}
