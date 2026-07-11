//! Direct, minimal cgroup v2 file I/O. Deliberately not a wrapper crate: cgroup v2's whole
//! design point is that it's just a virtual filesystem (a directory is a cgroup; a file in it is
//! a knob or a stat), so hand-rolling the handful of operations Hyperion actually needs is more
//! transparent and has one fewer dependency to track than pulling in a general-purpose cgroups
//! library for what `std::fs` already does directly.

use std::path::{Path, PathBuf};

use crate::CgroupError;

/// A real cgroup v2 directory: created on [`Cgroup::create`], always a child of some existing,
/// already-delegated parent (this crate never assumes it can write anywhere under
/// `/sys/fs/cgroup` — see the crate-level docs on delegation).
pub struct Cgroup {
    path: PathBuf,
}

impl Cgroup {
    /// Creates a real cgroup directory at `parent/name`. `parent` must already exist and be
    /// writable by this process (i.e. already delegated, by systemd or by a real root-owned
    /// init) — this never attempts to create or take over a parent cgroup itself.
    pub fn create(parent: impl AsRef<Path>, name: &str) -> Result<Self, CgroupError> {
        let path = parent.as_ref().join(name);
        std::fs::create_dir(&path).map_err(|source| CgroupError::Create {
            path: path.clone(),
            source,
        })?;
        Ok(Cgroup { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Moves the calling process itself into this cgroup — the standard cgroup v2 idiom for a
    /// process joining its own target cgroup (writing "0" to `cgroup.procs` means "the writing
    /// process's own pid"), avoiding the race a parent moving a just-forked child in from outside
    /// would have against that child's own first instructions.
    pub fn join_self(&self) -> Result<(), CgroupError> {
        self.write_control_file("cgroup.procs", "0")
    }

    /// Moves an arbitrary real, already-running process into this cgroup.
    pub fn add_process(&self, pid: libc::pid_t) -> Result<(), CgroupError> {
        self.write_control_file("cgroup.procs", &pid.to_string())
    }

    pub fn write_control_file(&self, file: &str, value: &str) -> Result<(), CgroupError> {
        let path = self.path.join(file);
        std::fs::write(&path, value).map_err(|source| CgroupError::WriteControl {
            path,
            value: value.to_string(),
            source,
        })
    }

    pub fn read_control_file(&self, file: &str) -> Result<String, CgroupError> {
        let path = self.path.join(file);
        std::fs::read_to_string(&path).map_err(|source| CgroupError::ReadControl { path, source })
    }

    /// Parses `cpu.stat`'s `usage_usec` field — real, kernel-accounted microseconds of CPU time
    /// this cgroup's processes have actually consumed, the measurement M4's exit criteria asks
    /// for ("measured from `/sys/fs/cgroup` accounting, not just from in-memory ledger state").
    pub fn cpu_usage_usec(&self) -> Result<u64, CgroupError> {
        let stat = self.read_control_file("cpu.stat")?;
        stat.lines()
            .find_map(|line| line.strip_prefix("usage_usec "))
            .and_then(|v| v.trim().parse().ok())
            .ok_or_else(|| CgroupError::MalformedStat {
                path: self.path.join("cpu.stat"),
            })
    }
}

impl Drop for Cgroup {
    fn drop(&mut self) {
        // Best-effort: a cgroup directory can only be rmdir'd once it has no processes left in
        // it (the kernel enforces this), so this is a no-op cleanup convenience for the common
        // "processes already exited" case, not a substitute for the caller reaping/waiting on
        // whatever it put in here.
        let _ = std::fs::remove_dir(&self.path);
    }
}
