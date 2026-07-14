//! Translates `hyperion_scheduler`'s admission/fairness decisions into real cgroup v2
//! configuration. Reuses the scheduler's own admission-control and fairness math as-is
//! (docs/998-roadmap.md M4's reuse map) — this module decides what real cgroup knobs
//! *express* a decision the scheduler already made, never a second fairness computation of its
//! own that could disagree with it.

use hyperion_scheduler::ResourceVector;

/// cgroup v2's own valid range for `cpu.weight`
/// (`Documentation/admin-guide/cgroup-v2.rst`).
const CPU_WEIGHT_MIN: u32 = 1;
const CPU_WEIGHT_MAX: u32 = 10_000;
/// cgroup v2's own default `cpu.weight`. A `priority_weight` of `1.0` (docs/04's baseline,
/// "an ordinary, unweighted task") maps to exactly this, so equal-weight tasks get equal real
/// CFS shares — matching DRF's own equal-weight baseline, not a different notion of "equal."
const CPU_WEIGHT_DEFAULT: u32 = 100;

/// A generous, fixed ceiling on process count per task cgroup. `ResourceVector` has no
/// process-count dimension for the scheduler to size this from, so this is defense-in-depth
/// against a runaway fork bomb, not a per-task scheduling decision.
pub const DEFAULT_PIDS_MAX: u32 = 512;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CgroupConfig {
    /// `cpu.weight`: cgroup v2's CFS proportional-share knob, 1-10000. This is what makes
    /// InteractiveAgent's higher `priority_weight` actually win more *real* CPU time than
    /// BackgroundAgent's, not just a higher rank in an in-memory sort.
    pub cpu_weight: u32,
    /// `memory.max` in bytes, or `None` for cgroup v2's "max" (unlimited) — derived from
    /// `ResourceVector::ram_mb`.
    pub memory_max_bytes: Option<u64>,
    /// `pids.max`, or `None` for "max" — see [`DEFAULT_PIDS_MAX`]'s docs for why this is fixed
    /// rather than derived from the request.
    pub pids_max: Option<u32>,
}

/// Maps a DRF `priority_weight` (docs/04-scheduler.md's dimensionless multiplier, where `1.0` is
/// the baseline) onto `cpu.weight`'s 1-10000 range, linearly around cgroup v2's own default: a
/// `priority_weight` of `2.0` gets exactly twice the real `cpu.weight` of `1.0`. This is the same
/// weight `Scheduler::dominant_share` already ranks candidates by — the OS mechanism enforces
/// what the algorithm decided, it does not recompute a different fairness notion of its own.
///
/// Checks `is_finite()` on the raw input *before* any arithmetic, deliberately: `f32::max`
/// resolves a NaN argument by returning the *other* one (`f32::NAN.max(0.0)` is `0.0`, not
/// NaN), so checking finiteness only after multiplying would silently treat NaN and `+Infinity`
/// input inconsistently (one would quietly collapse to the minimum weight, the other to the
/// default) — both are equally "not a real weight" and should map the same way.
pub fn cpu_weight_for(priority_weight: f32) -> u32 {
    if !priority_weight.is_finite() {
        return CPU_WEIGHT_DEFAULT;
    }
    let raw = (priority_weight.max(0.0) * CPU_WEIGHT_DEFAULT as f32).round();
    (raw as u32).clamp(CPU_WEIGHT_MIN, CPU_WEIGHT_MAX)
}

/// `ResourceVector::ram_mb` -> `memory.max` bytes. `0` maps to `None` ("max"/unlimited): a task
/// that didn't ask for any RAM budget at all is asking to be unconstrained on this dimension,
/// not constrained to zero (which would make it unable to allocate anything and immediately
/// OOM-kill).
pub fn memory_max_bytes_for(ram_mb: u32) -> Option<u64> {
    if ram_mb == 0 {
        None
    } else {
        Some(u64::from(ram_mb) * 1024 * 1024)
    }
}

/// Formats an `io.max` line for `ResourceVector::storage_iops`, cgroup v2's per-device I/O
/// bandwidth/IOPS control (`Documentation/admin-guide/cgroup-v2.rst`'s `io` controller).
///
/// Deliberately format-only: `io.max` is written *per block device* (keyed by its
/// `major:minor`), and this crate has no real block device to target yet — that arrives with
/// M6's real storage. `io` also isn't a controller this sandbox's own cgroup delegation exposes
/// at all (verified directly: `cgroup.controllers` at this sandbox's delegated cgroup lists only
/// `cpu memory pids`, not `io` — a WSL2/this-environment limitation, not a Hyperion one). Rather
/// than silently skip the `io` controller the roadmap explicitly names, or guess at a device to
/// write to, the mapping is implemented and unit-tested for its output *format* now; wiring it
/// to a real device's major:minor and actually writing it is M6's job, not invented early against
/// a device this milestone can't observe.
pub fn io_max_line_for(major: u32, minor: u32, iops: u32) -> Option<String> {
    if iops == 0 {
        None
    } else {
        Some(format!("{major}:{minor} riops={iops} wiops={iops}"))
    }
}

pub fn config_for(priority_weight: f32, request: ResourceVector) -> CgroupConfig {
    CgroupConfig {
        cpu_weight: cpu_weight_for(priority_weight),
        memory_max_bytes: memory_max_bytes_for(request.ram_mb),
        pids_max: Some(DEFAULT_PIDS_MAX),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_weight_maps_to_cgroup_v2_default() {
        assert_eq!(cpu_weight_for(1.0), CPU_WEIGHT_DEFAULT);
    }

    #[test]
    fn double_priority_weight_is_double_cpu_weight() {
        assert_eq!(cpu_weight_for(2.0), CPU_WEIGHT_DEFAULT * 2);
    }

    #[test]
    fn weight_clamps_to_cgroup_v2s_valid_range() {
        assert_eq!(cpu_weight_for(0.0), CPU_WEIGHT_MIN);
        assert_eq!(cpu_weight_for(1000.0), CPU_WEIGHT_MAX);
        assert_eq!(cpu_weight_for(f32::INFINITY), CPU_WEIGHT_DEFAULT);
        assert_eq!(cpu_weight_for(f32::NAN), CPU_WEIGHT_DEFAULT);
    }

    #[test]
    fn zero_ram_request_means_unlimited_not_zero() {
        assert_eq!(memory_max_bytes_for(0), None);
    }

    #[test]
    fn ram_request_converts_mb_to_bytes() {
        assert_eq!(memory_max_bytes_for(10), Some(10 * 1024 * 1024));
    }

    #[test]
    fn zero_iops_request_means_unlimited_not_zero() {
        assert_eq!(io_max_line_for(8, 0, 0), None);
    }

    #[test]
    fn io_max_line_matches_cgroup_v2s_expected_format() {
        assert_eq!(
            io_max_line_for(8, 0, 500),
            Some("8:0 riops=500 wiops=500".to_string())
        );
    }
}
