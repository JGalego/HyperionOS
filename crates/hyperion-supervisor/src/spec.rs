use std::path::PathBuf;

use hyperion_capability::RightsMask;
use hyperion_scheduler::ResourceVector;
use hyperion_trust_boundary::TrustDepth;

/// Real cgroup v2 enforcement (M4) to apply to a service's cpu/memory/pids, expressed the same
/// way `hyperion_cgroups::enforce_admission` already takes it. Kept optional on [`ServiceSpec`],
/// not required: a service whose real cgroup can't be created (e.g. this process itself isn't
/// inside a delegated cgroup subtree -- see `hyperion-cgroups`'s own docs on why that precondition
/// exists) should still be supervised and run, just without a real scheduling weight, rather than
/// refuse to start over a best-effort resource control failing. A degraded-but-running system
/// beats one that won't boot because a nice-to-have didn't apply.
#[derive(Debug, Clone, Copy)]
pub struct ServiceScheduling {
    pub priority_weight: f32,
    pub request: ResourceVector,
}

/// Everything the supervisor needs to spawn (and later, respawn) one capability-scoped, real
/// Linux process for a Phase 2-10 subsystem: the program to run, what real Trust Boundary
/// enforcement (M2) it gets spawned under, and (optionally) what real cgroup v2 configuration
/// (M4) governs its CPU/RAM. `name` is this service's stable identity across restarts -- the
/// thing a caller (or a test) looks it up by; it never changes even though the real pid and
/// capability token do, on every respawn.
#[derive(Debug, Clone)]
pub struct ServiceSpec {
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    /// The rights this service's spawn-time capability token grants -- re-minted fresh (a new
    /// `token_id`, generation 0) on every spawn and respawn, never reused, per M5's exit
    /// criterion ("restarted... with a fresh capability grant").
    pub rights: RightsMask,
    pub depth: TrustDepth,
    pub fs_scope: PathBuf,
    pub scheduling: Option<ServiceScheduling>,
    /// Extra environment variables set on every spawn *and* every respawn, alongside the
    /// supervisor's own `HYPERION_WIRE_TOKEN`/`HYPERION_IPC_SOCK` -- e.g. a test's
    /// `HYPERION_STATE_FILE` hook for observing a service instance's real, per-restart state from
    /// outside it. Any path a value here names must live inside `fs_scope` if the service intends
    /// to write to it: Landlock enforces exactly that scope, this field carries no exemption from
    /// it.
    pub extra_env: Vec<(String, String)>,
}
