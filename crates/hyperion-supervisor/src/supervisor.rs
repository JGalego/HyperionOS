//! The real supervision-tree mechanism: mint a fresh capability grant, spawn a real Trust
//! Boundary process (M2) for it, optionally place it in a real cgroup (M4), then watch for it to
//! exit and respawn it -- fresh grant, fresh process -- exactly like [`hyperion_recovery`]'s
//! already-real microreboot semantics, rehosted onto a real OS process instead of an in-process
//! Agent instance (see the roadmap's reuse map).
//!
//! Every child this process forks -- both capability-scoped services and the one plain,
//! unsandboxed process [`Supervisor::adopt_plain`] lets a caller register (`hyperion-init`'s
//! carryover debug shell; see that crate) -- is reaped through exactly one code path,
//! [`Supervisor::reap_and_restart_one`]'s single blocking `waitpid(-1, ...)` call. This is
//! deliberate, not incidental: if a second, independent `waitpid` call existed anywhere else in
//! this process (e.g. a separate thread blocking on one specific child's pid), it would race the
//! kernel's single wait-queue against this one for the same exited children, with no ordering
//! guarantee over which caller actually reaps a given child -- a real correctness hazard, not a
//! style preference. A single-threaded, single-waiter supervisor sidesteps it entirely, the same
//! way every real init system (`runit`, `s6`, `systemd`) funnels all child-reaping through one
//! place.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use hyperion_capability::{CapabilityMonitor, CapabilityToken, TrustBoundaryId, WireToken};
use hyperion_cgroups::Cgroup;
use hyperion_trust_boundary::{SpawnGrant, SpawnedBoundary};

use crate::errors::SupervisorError;
use crate::spec::ServiceSpec;

/// A respawn faster than this counts as a "fast failure" for backoff purposes -- moved here
/// unchanged from `hyperion-init`'s M1 placeholder loop, which this crate's [`Supervisor`]
/// replaces (see that crate's own docs).
const FAST_FAILURE_THRESHOLD: Duration = Duration::from_secs(2);

/// Capped exponential backoff so a child that fails instantly doesn't spin the supervisor at
/// 100% CPU. Applies uniformly to every tracked child (capability-scoped services and the plain
/// carryover shell alike) -- crash-looping is a process-lifecycle concern, not something that
/// should behave differently by child kind.
fn backoff_duration(consecutive_fast_failures: u32) -> Duration {
    let capped_exponent = consecutive_fast_failures.min(5);
    Duration::from_millis(200 * 2u64.pow(capped_exponent))
}

struct Bookkeeping {
    name: String,
    started_at: Instant,
    consecutive_fast_failures: u32,
    restart_count: u32,
}

enum ChildKind {
    /// A Phase 2-10 subsystem, spawned under a real, minted capability grant and real Trust
    /// Boundary enforcement (M2). `cgroup` is `None` when the service has no
    /// [`crate::ServiceScheduling`], or when one was requested but couldn't be applied for real
    /// (see [`Supervisor::spawn_sandboxed_inner`]'s docs on why that degrades rather than blocks
    /// startup) -- kept alive here only so its `Drop` cleans up the real cgroup directory once
    /// this entry is replaced or dropped, not because anything currently reads it back.
    Sandboxed {
        spec: Box<ServiceSpec>,
        boundary: SpawnedBoundary,
        token: CapabilityToken,
        origin: TrustBoundaryId,
        cgroup: Option<Cgroup>,
    },
    /// An already-spawned, unsandboxed process a caller wants folded into this same supervision
    /// tree (`hyperion-init`'s carryover debug shell is the one real user) -- see this module's
    /// own docs on why this can't safely be a second, independent wait loop instead.
    Plain {
        respawn: Box<dyn FnMut() -> io::Result<libc::pid_t>>,
    },
}

struct TrackedChild {
    book: Bookkeeping,
    kind: ChildKind,
}

/// The real supervision tree: one [`CapabilityMonitor`] (the sole authority minting/revoking
/// every supervised service's capability grant, matching docs/03's single-monitor model) plus a
/// table of every real child process currently being watched, keyed by its current, real pid.
///
/// A pid is not a stable identity across a restart (a respawn is a real `fork`, a real new pid);
/// [`ServiceSpec::name`] is. Every lookup a caller (or a test) does by name, not pid.
pub struct Supervisor {
    monitor: CapabilityMonitor,
    ipc_rendezvous_dir: PathBuf,
    cgroup_parent: Option<PathBuf>,
    next_origin: u64,
    children: HashMap<libc::pid_t, TrackedChild>,
}

impl Supervisor {
    /// `ipc_rendezvous_dir` is M5's concrete answer to the "real service-discovery directory"
    /// `hyperion-ipc`'s own M3 docs named as a deferred, separate piece of infrastructure: every
    /// spawned service is told its own well-known bind path beneath this directory
    /// (`HYPERION_IPC_SOCK`, see [`Self::socket_path_for`]), so a peer that knows a service's name
    /// knows exactly where it *would* reach it, without a shared in-process table. Nothing
    /// currently binds there for real yet -- see this crate's own docs on the seccomp/Landlock
    /// socket-rights extension that still needs to land first. `cgroup_parent` is `None` when no
    /// real, delegated cgroup subtree is available in this environment (real cgroup enforcement
    /// then degrades to "unweighted but running," never "refuses to start" -- see
    /// [`Self::spawn_sandboxed_inner`]).
    pub fn new(
        ipc_rendezvous_dir: impl Into<PathBuf>,
        cgroup_parent: Option<PathBuf>,
    ) -> Result<Self, SupervisorError> {
        let ipc_rendezvous_dir = ipc_rendezvous_dir.into();
        std::fs::create_dir_all(&ipc_rendezvous_dir).map_err(|source| {
            SupervisorError::RendezvousDir {
                path: ipc_rendezvous_dir.display().to_string(),
                source,
            }
        })?;
        Ok(Supervisor {
            monitor: CapabilityMonitor::new(),
            ipc_rendezvous_dir,
            cgroup_parent,
            next_origin: 1,
            children: HashMap::new(),
        })
    }

    fn socket_path_for(&self, name: &str) -> PathBuf {
        self.ipc_rendezvous_dir.join(format!("{name}.sock"))
    }

    /// Mints this service's first real capability grant, spawns it as a real Trust Boundary
    /// process, and starts supervising it. Every subsequent respawn (see
    /// [`Self::reap_and_restart_one`]) repeats the same mint-then-spawn sequence with a *fresh*
    /// token under the same `origin` -- established once, here, and reused for this service's
    /// whole lifetime so its provenance lineage stays identifiable across restarts even though
    /// its `token_id`/generation never are.
    pub fn spawn_sandboxed(&mut self, spec: ServiceSpec) -> Result<(), SupervisorError> {
        let origin = TrustBoundaryId(self.next_origin);
        self.next_origin += 1;
        let (pid, boundary, token, cgroup) = self.spawn_sandboxed_inner(&spec, origin)?;
        self.children.insert(
            pid,
            TrackedChild {
                book: Bookkeeping {
                    name: spec.name.clone(),
                    started_at: Instant::now(),
                    consecutive_fast_failures: 0,
                    restart_count: 0,
                },
                kind: ChildKind::Sandboxed {
                    spec: Box::new(spec),
                    boundary,
                    token,
                    origin,
                    cgroup,
                },
            },
        );
        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn spawn_sandboxed_inner(
        &mut self,
        spec: &ServiceSpec,
        origin: TrustBoundaryId,
    ) -> Result<
        (
            libc::pid_t,
            SpawnedBoundary,
            CapabilityToken,
            Option<Cgroup>,
        ),
        SupervisorError,
    > {
        let token = self.monitor.mint_root(spec.rights, origin, None);
        let grant = SpawnGrant {
            token: token.clone(),
            depth: spec.depth,
            fs_scope: spec.fs_scope.clone(),
        };

        let mut command = Command::new(&spec.program);
        command.args(&spec.args);
        command.env(
            "HYPERION_WIRE_TOKEN",
            serde_json::to_string(&WireToken::from(&token))
                .expect("WireToken serialization never fails: it is plain data, no fallible step"),
        );
        command.env("HYPERION_IPC_SOCK", self.socket_path_for(&spec.name));
        command.envs(spec.extra_env.iter().map(|(k, v)| (k.as_str(), v.as_str())));

        let boundary = hyperion_trust_boundary::spawn(&grant, command).map_err(|source| {
            SupervisorError::Spawn {
                name: spec.name.clone(),
                source,
            }
        })?;
        let pid = boundary.pid();

        let cgroup = self.try_apply_cgroup(spec, pid);
        Ok((pid, boundary, token, cgroup))
    }

    /// Real cgroup v2 enforcement (M4) for one service, applied best-effort: a failure here
    /// (most commonly, this process isn't itself inside a delegated cgroup subtree -- see
    /// `hyperion-cgroups`'s own docs on that precondition) is logged and the service still runs,
    /// just unweighted, rather than treated as a reason to refuse to start it. Real production
    /// boot (real root, PID 1) doesn't hit this at all: real root can create and join cgroups
    /// anywhere under `/sys/fs/cgroup` directly, delegation only restricts *unprivileged* uids.
    fn try_apply_cgroup(&self, spec: &ServiceSpec, pid: libc::pid_t) -> Option<Cgroup> {
        let scheduling = spec.scheduling?;
        let parent = self.cgroup_parent.as_ref()?;
        match hyperion_cgroups::enforce_admission(
            parent,
            &spec.name,
            scheduling.priority_weight,
            scheduling.request,
        ) {
            Ok(cgroup) => {
                if let Err(e) = cgroup.add_process(pid) {
                    eprintln!(
                        "hyperion-supervisor: warning: {} spawned but could not join its real \
                         cgroup ({e}) -- running unweighted",
                        spec.name
                    );
                }
                Some(cgroup)
            }
            Err(e) => {
                eprintln!(
                    "hyperion-supervisor: warning: real cgroup for {} unavailable ({e}) -- \
                     running unweighted, not refusing to start",
                    spec.name
                );
                None
            }
        }
    }

    /// Folds an already-spawned, unsandboxed process into this same supervision tree so it's
    /// reaped and restarted through the same single wait loop as every capability-scoped service
    /// (see this module's own docs on why a second, independent waiter would be unsafe). Real
    /// user: `hyperion-init`'s carryover interactive debug shell, which is deliberately *not*
    /// wrapped in a [`crate::ServiceSpec`] -- a shell exists to let a human debug a real boot by
    /// hand and fundamentally needs broad access to be useful for that, unlike a Phase 2-10
    /// subsystem; M7 is what gives the booted system a real, properly capability-scoped
    /// interactive surface, not this crate.
    pub fn adopt_plain(
        &mut self,
        name: impl Into<String>,
        initial_pid: libc::pid_t,
        respawn: impl FnMut() -> io::Result<libc::pid_t> + 'static,
    ) {
        self.children.insert(
            initial_pid,
            TrackedChild {
                book: Bookkeeping {
                    name: name.into(),
                    started_at: Instant::now(),
                    consecutive_fast_failures: 0,
                    restart_count: 0,
                },
                kind: ChildKind::Plain {
                    respawn: Box::new(respawn),
                },
            },
        );
    }

    /// Blocks until any one supervised child exits, then restarts exactly that one -- a
    /// capability-scoped service gets a brand-new mint-then-spawn (a fresh `token_id`, never the
    /// dead process's own, per M5's exit criterion), a plain adopted process just gets
    /// re-invoked. Every other tracked child is untouched: this is what "without crashing sibling
    /// services" means concretely -- each lives in its own real process and its own table entry,
    /// so restarting one never touches another's.
    ///
    /// Returns the restarted service's name, or `""` if the reaped pid belonged to an untracked
    /// grandchild orphan (reparented here rather than a direct child of this process) -- not
    /// something to restart, and not a reason for PID 1 to abort.
    ///
    /// If the respawn attempt itself fails (distinct from the original process merely exiting),
    /// this service is logged and dropped from supervision rather than retried indefinitely -- a
    /// real give-up/alerting policy for "even the fresh spawn attempt keeps failing" is a real,
    /// separate refinement this MVP milestone doesn't attempt (mirrors M4's own documented
    /// `io.max`/`SCHED_DEADLINE` deferrals: implemented for the case the exit criteria actually
    /// tests, not silently pretended to handle every failure mode beyond it).
    pub fn reap_and_restart_one(&mut self) -> Result<String, SupervisorError> {
        let mut status: libc::c_int = 0;
        // SAFETY: -1 waits for any child of this process; status is a valid out-pointer.
        let pid = unsafe { libc::waitpid(-1, &mut status, 0) };
        if pid < 0 {
            return Err(SupervisorError::Wait(io::Error::last_os_error()));
        }

        let Some(mut child) = self.children.remove(&pid) else {
            return Ok(String::new());
        };

        let name = child.book.name.clone();
        let fast_failure = child.book.started_at.elapsed() < FAST_FAILURE_THRESHOLD;
        child.book.consecutive_fast_failures = if fast_failure {
            child.book.consecutive_fast_failures + 1
        } else {
            0
        };
        std::thread::sleep(backoff_duration(child.book.consecutive_fast_failures));

        let new_pid = match &mut child.kind {
            ChildKind::Sandboxed {
                spec,
                boundary,
                token,
                origin,
                cgroup,
            } => {
                // The process is already gone (that's why waitpid just returned it) -- this
                // fences off any *other* holder of the same now-stale token, it doesn't kill
                // anything.
                self.monitor.cap_revoke(token);
                let (new_pid, new_boundary, new_token, new_cgroup) =
                    self.spawn_sandboxed_inner(spec, *origin)?;
                *token = new_token;
                *boundary = new_boundary;
                *cgroup = new_cgroup;
                new_pid
            }
            ChildKind::Plain { respawn } => respawn().map_err(|source| SupervisorError::Spawn {
                name: name.clone(),
                source,
            })?,
        };

        child.book.restart_count += 1;
        child.book.started_at = Instant::now();
        self.children.insert(new_pid, child);
        Ok(name)
    }

    /// `hyperion-init`'s real PID 1 usage: supervise forever, logging each restart. Never
    /// returns -- the one thing that could stop this is `reap_and_restart_one` itself failing to
    /// even call `waitpid` (a kernel-level problem with this process, not a child crashing),
    /// which is logged and does not stop the loop either, since a supervisor that gives up on
    /// its own boot process over one transient wait error is worse than one that keeps trying.
    pub fn run_forever(&mut self) -> ! {
        loop {
            match self.reap_and_restart_one() {
                Ok(name) if !name.is_empty() => {
                    println!("[hyperion-supervisor] restarted {name}");
                }
                Ok(_) => {}
                Err(e) => eprintln!("[hyperion-supervisor] wait/restart error: {e}"),
            }
        }
    }

    pub fn pid_of(&self, name: &str) -> Option<libc::pid_t> {
        self.children
            .iter()
            .find(|(_, c)| c.book.name == name)
            .map(|(&pid, _)| pid)
    }

    /// The current live capability token's id for a sandboxed service -- `None` for a name that
    /// doesn't exist, or that names a plain adopted process (which has no capability grant at
    /// all). Two calls returning different values for the same `name` is exactly what "restarted
    /// with a fresh capability grant" looks like from the outside.
    pub fn token_id_of(&self, name: &str) -> Option<u64> {
        self.children.values().find_map(|c| {
            if c.book.name != name {
                return None;
            }
            match &c.kind {
                ChildKind::Sandboxed { token, .. } => Some(token.token_id().0),
                ChildKind::Plain { .. } => None,
            }
        })
    }

    pub fn restart_count_of(&self, name: &str) -> Option<u32> {
        self.children
            .values()
            .find(|c| c.book.name == name)
            .map(|c| c.book.restart_count)
    }

    /// Kills and reaps every currently tracked child for real. Not part of the crash-detection
    /// path (which never wants to kill a process that's still healthy) -- this is test/shutdown
    /// cleanup, so a caller (a test, or a future real halt path) can tear down every real process
    /// this supervisor ever spawned or adopted without leaking them.
    pub fn shutdown(&mut self) {
        let children: Vec<(libc::pid_t, TrackedChild)> = self.children.drain().collect();
        for (pid, child) in children {
            match child.kind {
                ChildKind::Sandboxed { boundary, .. } => {
                    boundary.revoke(&mut self.monitor);
                }
                ChildKind::Plain { .. } => {
                    // SAFETY: `pid` is a real child this process forked (directly, or was told
                    // about via `adopt_plain`) and hasn't been reaped yet.
                    unsafe { libc::kill(pid, libc::SIGKILL) };
                    let mut status: libc::c_int = 0;
                    unsafe { libc::waitpid(pid, &mut status, 0) };
                }
            }
        }
    }
}

impl Drop for Supervisor {
    /// A live `Supervisor` going out of scope -- normal return, or unwinding from a panic in
    /// whatever it's supervising a test around -- always means "stop supervising these
    /// processes," never "abandon them and let them run forever": unlike [`SpawnedBoundary`]
    /// (a handle to *one* boundary, which a caller might legitimately want to drop without
    /// killing, e.g. handing ownership elsewhere), this type owns the *whole* tree, and nothing
    /// in this crate ever wants a dropped `Supervisor`'s real children to keep running
    /// unsupervised. Found the hard way: an earlier version of this crate's own test panicked on
    /// a bad assertion partway through, and without this, its two still-running sandboxed child
    /// processes were silently orphaned -- alive forever, holding the test harness's own stdout
    /// pipe open, so the test run never even appeared to finish.
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_caps() {
        let d0 = backoff_duration(0);
        let d1 = backoff_duration(1);
        let d5 = backoff_duration(5);
        let d50 = backoff_duration(50);

        assert!(
            d0 < d1,
            "backoff should grow with consecutive fast failures"
        );
        assert_eq!(d5, d50, "backoff should cap rather than grow unbounded");
        assert!(
            d50 < Duration::from_secs(30),
            "capped backoff should stay bounded"
        );
    }
}
