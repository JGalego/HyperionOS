/// Everything that can go wrong applying real enforcement to a spawned Trust Boundary process.
///
/// Deliberately its own type rather than reusing `hyperion_capability::Fault`: these are
/// OS-mechanism failures (a syscall failing, a kernel feature missing), a different failure
/// class from `Fault`'s capability-algorithm violations (revoked, expired, insufficient rights).
#[derive(Debug, thiserror::Error)]
pub enum EnforcementError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Landlock ruleset error: {0}")]
    Landlock(#[from] landlock::RulesetError),
    #[error("Landlock path error: {0}")]
    LandlockPath(#[from] landlock::PathFdError),
    #[error("seccomp filter error: {0}")]
    Seccomp(#[from] seccompiler::Error),
    #[error("seccomp backend error: {0}")]
    SeccompBackend(#[from] seccompiler::BackendError),
}
