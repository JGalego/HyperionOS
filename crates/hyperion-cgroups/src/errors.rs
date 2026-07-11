use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CgroupError {
    #[error("failed to create cgroup at {path}: {source}")]
    Create {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to write {value:?} to {path}: {source}")]
    WriteControl {
        path: PathBuf,
        value: String,
        source: std::io::Error,
    },
    #[error("failed to read {path}: {source}")]
    ReadControl {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} did not contain the expected stat field")]
    MalformedStat { path: PathBuf },
}

/// Everything that can go wrong applying a real-time scheduling policy — kept distinct from
/// [`CgroupError`] since these are raw syscall failures, not filesystem operations.
#[derive(Debug, thiserror::Error)]
pub enum RealtimeError {
    #[error("sched_setattr failed: {0}")]
    SchedSetattr(std::io::Error),
    #[error("sched_setscheduler failed: {0}")]
    SchedSetscheduler(std::io::Error),
}
