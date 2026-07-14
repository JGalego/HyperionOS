use std::io;

/// Why the supervisor stopped restarting a service. Lives here (rather than in the Linux-only
/// `supervisor` module alongside [`crate::GivenUpService`]) because [`SupervisorError`] itself
/// must stay available on every platform -- this crate's own convention (see this crate's own
/// `lib.rs`): `errors`/`spec` are plain data with no OS-specific dependency, so nothing in either
/// module may reference anything `#[cfg(target_os = "linux")]`-gated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GiveUpReason {
    /// Fast-failed too many times in a row: the process keeps starting and immediately dying.
    CrashLoop,
    /// The respawn attempt itself (a fresh mint-then-spawn, or the caller's own `respawn`
    /// closure) returned an error -- distinct from the process starting and then dying.
    RespawnFailed,
}

#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("failed to create the IPC rendezvous directory at {path}: {source}")]
    RendezvousDir { path: String, source: io::Error },
    #[error("failed to spawn service {name:?}: {source}")]
    Spawn { name: String, source: io::Error },
    #[error("failed to wait on a supervised child: {0}")]
    Wait(io::Error),
    /// The supervisor has stopped trying to restart `name` -- see [`GiveUpReason`]. Returned
    /// instead of a restart, never silently: a caller (or `run_forever`'s own log) always sees
    /// *why* a service stopped running.
    #[error(
        "gave up restarting service {name:?} after {restart_count} prior restart(s): {reason:?}"
    )]
    GaveUp {
        name: String,
        restart_count: u32,
        reason: GiveUpReason,
    },
}
