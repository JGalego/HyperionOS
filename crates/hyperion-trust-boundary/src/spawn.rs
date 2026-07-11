use std::io;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RevocationReceipt};

use crate::enforcement::{apply_landlock, apply_namespaces, apply_seccomp};
use crate::types::{SpawnGrant, TrustDepth};

/// A real, separate Linux process spawned as a Trust Boundary, plus enough state to revoke it.
pub struct SpawnedBoundary {
    pid: libc::pid_t,
    token: CapabilityToken,
}

/// Spawns `command` as a real Trust Boundary process: forks, applies `grant`'s real enforcement
/// (namespaces if `depth == Container`, then Landlock, then seccomp, in that order -- seccomp
/// must be last, since once installed it can forbid the syscalls the earlier steps still need),
/// then execs `command`. `command` carries the program, args, and any env/cwd the caller needs
/// (Landlock/seccomp restrict the process, not what it's told to run). The parent gets back a
/// handle to observe or revoke the child; the enforcement itself is *self*-applied by the child,
/// per Landlock/seccomp's own design (see `enforcement`'s module docs).
pub fn spawn(grant: &SpawnGrant, mut command: Command) -> io::Result<SpawnedBoundary> {
    let rights = grant.token.rights();
    let depth = grant.depth;
    let fs_scope = grant.fs_scope.clone();
    // Extracted before the fork/move below: Landlock needs the program's own path to grant it
    // read+execute access independent of whatever `rights` governs on `fs_scope` (see
    // `apply_landlock`'s docs for why those are two different concerns).
    let program_path = PathBuf::from(command.get_program());

    // SAFETY: fork() duplicates the process. The child branch below only calls
    // async-signal-safe setup (the unshare/Landlock/seccomp syscalls) and then either execs or
    // _exit()s -- it never returns into Rust-level state shared with the parent.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(io::Error::last_os_error());
    }
    if pid == 0 {
        let outcome = (|| -> io::Result<io::Error> {
            if depth == TrustDepth::Container {
                apply_namespaces().map_err(to_io_error)?;
            }
            apply_landlock(&fs_scope, rights, &program_path).map_err(to_io_error)?;
            apply_seccomp().map_err(to_io_error)?;
            // exec() only returns (as an Err) on failure -- success replaces this process image.
            Ok(command.exec())
        })();
        match outcome {
            Ok(exec_err) => eprintln!("hyperion-trust-boundary: exec failed: {exec_err}"),
            Err(setup_err) => {
                eprintln!("hyperion-trust-boundary: sandbox setup failed: {setup_err}")
            }
        }
        // The parent is still waiting on this pid; never fall back to running the target
        // program unsandboxed.
        std::process::exit(127);
    }

    Ok(SpawnedBoundary {
        pid,
        token: grant.token.clone(),
    })
}

fn to_io_error<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::other(e.to_string())
}

impl SpawnedBoundary {
    pub fn pid(&self) -> libc::pid_t {
        self.pid
    }

    /// True iff the process still exists (and is signalable by us) -- a plain existence probe,
    /// sending no actual signal.
    pub fn is_alive(&self) -> bool {
        // SAFETY: signal 0 sends nothing; it only reports whether the pid is signalable.
        unsafe { libc::kill(self.pid, 0) == 0 }
    }

    /// The real revocation effect M2 requires: kills the process outright (Landlock/seccomp are
    /// install-once, self-restricting mechanisms with no live "narrow this one right" callback,
    /// so termination -- not a partial access downgrade -- is what "revoked" really means for an
    /// already-sandboxed process), reaps it so it doesn't linger as a zombie, and revokes the
    /// token in `monitor` so every other holder's `is_live` check reflects the same revocation.
    pub fn revoke(self, monitor: &mut CapabilityMonitor) -> RevocationReceipt {
        // SAFETY: `self.pid` is a real child this process forked and hasn't reaped yet.
        unsafe { libc::kill(self.pid, libc::SIGKILL) };
        let mut status: libc::c_int = 0;
        // SAFETY: status is a valid out-pointer; reaps the child killed above.
        unsafe { libc::waitpid(self.pid, &mut status, 0) };
        monitor.cap_revoke(&self.token)
    }
}

impl Drop for SpawnedBoundary {
    fn drop(&mut self) {
        let mut status: libc::c_int = 0;
        // SAFETY: WNOHANG makes this non-blocking and status is a valid out-pointer; reaps the
        // child only if it has already exited on its own, so dropping a handle to a boundary
        // that's still running never blocks on it or kills it.
        unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
    }
}
