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
    /// sending no actual signal. **Not** an "has it finished" check: an exited-but-unreaped child
    /// is a real zombie, still a valid, signalable process table entry, so this stays `true` until
    /// something actually reaps it (`wait`, `kill`, `revoke`, or `Drop`'s own best-effort
    /// `WNOHANG`) -- a caller polling for real completion wants [`Self::try_wait`] instead, which
    /// this crate's own earlier real bug (an infinite poll loop in
    /// `hyperion-plugin-framework::registry::invoke_native_binary`, caught live) is the reason
    /// this doc comment is this explicit.
    pub fn is_alive(&self) -> bool {
        // SAFETY: signal 0 sends nothing; it only reports whether the pid is signalable.
        unsafe { libc::kill(self.pid, 0) == 0 }
    }

    /// A real, non-blocking check for real completion -- unlike [`Self::is_alive`] (stays `true`
    /// for an unreaped zombie), this actually reaps the child the moment it exits, returning
    /// `Some(exit_code)` exactly once, or `None` while it's still genuinely running. Mirrors
    /// `std::process::Child::try_wait`'s own well-known shape. The real, correct way to poll for a
    /// bounded wait: loop calling this, sleep between calls, and stop looping (calling
    /// [`Self::kill`] instead) once your own deadline passes.
    pub fn try_wait(&mut self) -> io::Result<Option<i32>> {
        let mut status: libc::c_int = 0;
        // SAFETY: `self.pid` is a real child this process forked and hasn't reaped yet; status is
        // a valid out-pointer; WNOHANG makes this non-blocking.
        let reaped = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
        if reaped == 0 {
            return Ok(None);
        }
        if reaped < 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::WIFEXITED(status) {
            Ok(Some(libc::WEXITSTATUS(status)))
        } else {
            Err(io::Error::other(format!(
                "sandboxed process (pid {}) did not exit normally (raw status {status})",
                self.pid
            )))
        }
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

    /// A real, blocking wait for this real child to exit on its own -- unlike [`Self::is_alive`]
    /// (a non-blocking probe) or `Drop`'s own `WNOHANG` reap (only collects an *already*-exited
    /// child), this actually blocks until the sandboxed program finishes, then returns its real
    /// exit code. Consumes `self`, mirroring [`Self::revoke`]'s own ownership shape: once reaped,
    /// this handle no longer refers to a live process either way. A caller that instead wants a
    /// bounded wait polls [`Self::is_alive`] and calls [`Self::kill`] on timeout, never blocking
    /// here in the first place -- see `hyperion-plugin-framework::registry::invoke_native_binary`.
    pub fn wait(self) -> io::Result<i32> {
        let mut status: libc::c_int = 0;
        // SAFETY: `self.pid` is a real child this process forked and hasn't reaped yet; status is
        // a valid out-pointer.
        if unsafe { libc::waitpid(self.pid, &mut status, 0) } < 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::WIFEXITED(status) {
            Ok(libc::WEXITSTATUS(status))
        } else {
            Err(io::Error::other(format!(
                "sandboxed process (pid {}) did not exit normally (raw status {status})",
                self.pid
            )))
        }
    }

    /// Kills and reaps this real child without touching any [`CapabilityMonitor`]'s revocation
    /// graph -- for a caller (like a bounded-timeout waiter) that has no monitor handle to hand
    /// [`Self::revoke`], and isn't revoking a capability so much as ending a process that ran too
    /// long. A holder of the same token elsewhere is unaffected; call `revoke` instead when that
    /// matters.
    pub fn kill(self) -> io::Result<()> {
        // SAFETY: `self.pid` is a real child this process forked and hasn't reaped yet.
        if unsafe { libc::kill(self.pid, libc::SIGKILL) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut status: libc::c_int = 0;
        // SAFETY: status is a valid out-pointer; reaps the child just killed above.
        unsafe { libc::waitpid(self.pid, &mut status, 0) };
        Ok(())
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
