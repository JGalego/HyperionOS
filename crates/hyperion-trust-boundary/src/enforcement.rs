//! The real OS-level mechanisms a spawned Trust Boundary is scoped by: namespaces, Landlock,
//! seccomp. Every function here runs *inside* the forked child, before it execs the target
//! program -- Landlock and seccomp are both "restrict self" mechanisms by design (that's what
//! makes them usable unprivileged), so there is no way to apply them to another process from
//! the outside.

use std::path::Path;

use hyperion_capability::RightsMask;
use landlock::{
    AccessFs, BitFlags, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset, RulesetAttr,
    RulesetCreatedAttr,
};
use seccompiler::{BpfProgram, SeccompAction, SeccompFilter, TargetArch};

use crate::errors::EnforcementError;

/// Maps `hyperion_capability::RightsMask` bits onto Landlock's filesystem access-rights,
/// deliberately field-by-field rather than via `AccessFs::from_read()`/`from_write()`: those
/// helpers bundle in more than their name suggests (`from_read()` includes `Execute`;
/// `from_write()` includes directory/device-creation rights like `MakeSock`/`MakeBlock`), which
/// would make e.g. a plain `READ` grant silently able to execute anything in scope too. Building
/// the set explicitly keeps this crate's `RightsMask` bits meaning exactly what they say.
///
/// `MAP` has no distinct Landlock right: mapping a file `PROT_READ`/`PROT_EXEC` is governed by
/// `ReadFile`/`Execute` respectively, so `MAP` is folded into whichever of those is already
/// granted rather than requiring its own bit. `GRANT`/`REVOKE` govern the capability-delegation
/// algorithm itself (who may call `cap_derive`/`cap_revoke`), not filesystem access, so they have
/// no OS-enforcement mapping here.
pub fn fs_access_for_rights(rights: RightsMask) -> BitFlags<AccessFs> {
    let mut access = BitFlags::<AccessFs>::empty();
    if rights.contains(RightsMask::READ) {
        access |= AccessFs::ReadFile | AccessFs::ReadDir;
    }
    if rights.contains(RightsMask::WRITE) {
        access |= AccessFs::WriteFile;
    }
    if rights.contains(RightsMask::EXEC) {
        access |= AccessFs::Execute;
    }
    access
}

/// Joins a fresh, unprivileged user namespace (mapping the caller's own uid/gid to 0 within it,
/// the standard "unprivileged user namespace" dance), then unshares the mount/net/uts/ipc
/// namespaces it that authority now permits. Depth 1 (`TrustDepth::Process`) never calls this;
/// only depth 2 (`TrustDepth::Container`) does.
pub fn apply_namespaces() -> Result<(), EnforcementError> {
    // SAFETY: unshare() is always valid to call; CLONE_NEWUSER creates a new, empty user
    // namespace that this (single-threaded, freshly forked) process joins alone.
    if unsafe { libc::unshare(libc::CLONE_NEWUSER) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }

    // Required before writing gid_map as an unprivileged process (CVE-2014-8989's fix): the
    // kernel refuses to map any gid, including via "deny", until setgroups is disabled first.
    std::fs::write("/proc/self/setgroups", b"deny")?;
    // SAFETY: getuid/getgid never fail.
    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };
    std::fs::write("/proc/self/uid_map", format!("0 {uid} 1"))?;
    std::fs::write("/proc/self/gid_map", format!("0 {gid} 1"))?;

    // Now privileged within our own user namespace, so the remaining unshares don't need host
    // root. CLONE_NEWPID is deliberately excluded -- see TrustDepth::Container's docs.
    let flags = libc::CLONE_NEWNS | libc::CLONE_NEWNET | libc::CLONE_NEWUTS | libc::CLONE_NEWIPC;
    // SAFETY: valid flag combination, called after gaining namespace-root above.
    if unsafe { libc::unshare(flags) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }

    Ok(())
}

/// Restricts the calling process's own filesystem access to exactly `fs_scope` (with exactly
/// the rights `rights` names) plus exactly enough access to `program_path` to load and run it.
/// This is the real effect a `CapabilityToken`'s `RightsMask` has on a real Trust Boundary
/// process. Landlock rulesets are strictly additive-only across the process's lifetime (a later
/// call can only narrow, never widen, what an earlier one restricted), which is exactly the
/// attenuation-only guarantee `hyperion_capability::cap_derive` already enforces at the
/// algorithm layer -- the OS mechanism and the capability algorithm agree by construction.
///
/// `program_path` gets its own rule, separate from and unconditional on `rights`: being able to
/// load and execute your own program is a precondition for a Trust Boundary to exist at all, not
/// a resource access `RightsMask` should have to name -- once `ReadFile` is a handled category
/// (because `RightsMask::READ` is set), the kernel's own read of the target program's ELF
/// content during `execve()` is checked exactly like any other read, so without a rule granting
/// it on `program_path` specifically (which typically lives outside `fs_scope`), the boundary
/// would fail to even start. Any right this boundary doesn't hold over `fs_scope` stays
/// ungoverned by Landlock there, not silently denied everywhere -- see [`fs_access_for_rights`].
///
/// `ipc_rendezvous`, when `Some`, is `hyperion_trust_boundary::SpawnGrant`'s own real, distinct
/// IPC-rights dimension: a real Landlock `MakeSock` rule scoped to the rendezvous path's *parent
/// directory* -- Landlock's creation-type rights (unlike `ReadFile`/`WriteFile`, which apply to a
/// thing that already exists) can only ever be scoped to a containing directory, since the thing
/// being created doesn't exist yet at rule-installation time, so this cannot be narrowed to
/// exactly the one socket filename the caller actually intends to bind. An honest, acknowledged
/// scope boundary, not a hidden one: a boundary granted IPC rights can really create a socket
/// *anywhere in that same rendezvous directory*, not provably only its own -- `hyperion-supervisor`'s
/// own single shared rendezvous directory per service makes this the real, achievable precision
/// Landlock's actual primitives allow, not a design shortcut taken here.
pub fn apply_landlock(
    fs_scope: &Path,
    rights: RightsMask,
    program_path: &Path,
    ipc_rendezvous: Option<&Path>,
) -> Result<(), EnforcementError> {
    let fs_scope_access = fs_access_for_rights(rights);
    let program_access = AccessFs::ReadFile | AccessFs::Execute;
    let ipc_access = if ipc_rendezvous.is_some() {
        BitFlags::<AccessFs>::from(AccessFs::MakeSock)
    } else {
        BitFlags::<AccessFs>::empty()
    };

    let mut created = Ruleset::default()
        .set_compatibility(CompatLevel::BestEffort)
        .handle_access(fs_scope_access | program_access | ipc_access)?
        .create()?
        .add_rule(PathBeneath::new(PathFd::new(program_path)?, program_access))?;

    if !fs_scope_access.is_empty() {
        created = created.add_rule(PathBeneath::new(PathFd::new(fs_scope)?, fs_scope_access))?;
    }

    if let Some(rendezvous) = ipc_rendezvous {
        let rendezvous_dir = rendezvous.parent().ok_or_else(|| {
            EnforcementError::Io(std::io::Error::other(format!(
                "ipc_rendezvous path {rendezvous:?} has no parent directory to scope a real \
                 Landlock MakeSock rule to"
            )))
        })?;
        created = created.add_rule(PathBeneath::new(PathFd::new(rendezvous_dir)?, ipc_access))?;
    }

    created.restrict_self()?;

    Ok(())
}

/// The baseline syscall allowlist for a simple, statically-linked Linux binary: enough to start
/// up, exec into (this filter is installed before the target's own `execve`), do straightforward
/// file/stdio I/O within whatever Landlock already scoped, and exit. Default action for anything
/// not on this list is `Errno(EPERM)` (see [`apply_seccomp`]), not a silent kill, so a denied
/// syscall shows up as an ordinary I/O error to the sandboxed program rather than a mysterious
/// crash -- except `poll`/`tkill`/`tgkill` specifically, found missing the hard way (via
/// `strace`, not guessed): the Rust runtime's startup polls fds 0/1/2 to detect closed standard
/// streams before `main()` runs, and reacts to *that* failing by self-signaling `SIGABRT` via
/// `tkill`, so omitting either turns one denied syscall into a runtime that can't even abort
/// cleanly and dies by `SIGSEGV` instead -- confusing to debug from the outside, since nothing
/// about a `SIGSEGV` points back at seccomp. Both `open` and `openat` are listed for the same
/// reason: this musl target's libc reaches for plain `open` in places glibc would use `openat`,
/// so allowlisting only the modern syscall silently breaks musl binaries specifically. `getpid` was
/// the next gap found the same way (docs/998-roadmap.md M5, this time via direct evidence
/// rather than `strace`): a sandboxed process calling `std::process::id()` got back `4294967295`
/// (`u32::MAX`) instead of its real pid -- musl's `getpid()` wrapper populates its per-process
/// cache by issuing the real syscall on first call (a fresh `exec` wipes any earlier cache), and
/// neither it nor `std::process::id()` checks for the syscall failing, so the denied call's `-1`
/// return silently became `u32::MAX` once cast. Every Trust Boundary process this crate spawns
/// legitimately needs to know its own pid at some point (if only to log it), so this belongs in
/// the baseline, not behind a `RightsMask` bit the way the still-deferred socket syscalls do (see
/// `hyperion-supervisor`'s own docs) -- unlike a socket, a process's own pid isn't a resource
/// access `RightsMask` governs at all.
///
/// docs/998-roadmap.md M11: `arch_prctl`, plain `open`, plain `access`, and plain `poll`
/// are x86_64-only entries, `#[cfg]`'d out on aarch64 below rather than merely unused there --
/// aarch64's syscall table is the newer, reduced "asm-generic" set that never had these legacy
/// dual forms to begin with (only `arch_prctl`'s job -- setting up the userspace thread pointer --
/// has no aarch64 syscall equivalent at all; the kernel sets `TPIDR_EL0` directly during `execve`
/// instead), so `libc` itself doesn't define these constants for that target and the crate would
/// fail to *compile* for aarch64, not just misbehave at runtime, if they stayed unconditional.
/// musl's aarch64 build already calls `openat`/`faccessat`/`ppoll` internally where its x86_64
/// build calls the now-`cfg`'d-out legacy forms, so the shared list below covers it without them.
fn baseline_allowed_syscalls() -> Vec<i64> {
    #[cfg_attr(not(target_arch = "x86_64"), allow(unused_mut))]
    let mut syscalls = vec![
        libc::SYS_getpid,
        libc::SYS_execve,
        libc::SYS_read,
        libc::SYS_write,
        libc::SYS_readv,
        libc::SYS_writev,
        libc::SYS_close,
        libc::SYS_fstat,
        libc::SYS_newfstatat,
        libc::SYS_lseek,
        libc::SYS_mmap,
        libc::SYS_mprotect,
        libc::SYS_munmap,
        libc::SYS_brk,
        libc::SYS_rt_sigaction,
        libc::SYS_rt_sigprocmask,
        libc::SYS_rt_sigreturn,
        libc::SYS_sigaltstack,
        libc::SYS_ioctl,
        libc::SYS_ppoll,
        libc::SYS_tkill,
        libc::SYS_tgkill,
        libc::SYS_faccessat,
        libc::SYS_faccessat2,
        libc::SYS_openat,
        libc::SYS_pread64,
        libc::SYS_pwrite64,
        libc::SYS_getrandom,
        libc::SYS_set_tid_address,
        libc::SYS_set_robust_list,
        libc::SYS_rseq,
        libc::SYS_prlimit64,
        libc::SYS_clock_gettime,
        libc::SYS_clock_nanosleep,
        libc::SYS_nanosleep,
        libc::SYS_futex,
        libc::SYS_madvise,
        libc::SYS_getcwd,
        libc::SYS_exit,
        libc::SYS_exit_group,
    ];

    #[cfg(target_arch = "x86_64")]
    syscalls.extend([
        libc::SYS_arch_prctl,
        libc::SYS_open,
        libc::SYS_access,
        libc::SYS_poll,
    ]);

    syscalls
}

/// The `seccompiler::TargetArch` matching the architecture this binary was actually compiled
/// for -- installing a filter compiled for the wrong arch is worse than installing none: the BPF
/// program's own arch-validation prologue (`build_arch_validation_sequence`) would reject every
/// real syscall the running process makes, since none of them would match the audit-arch value
/// baked into the filter.
#[cfg(target_arch = "x86_64")]
fn host_target_arch() -> TargetArch {
    TargetArch::x86_64
}

#[cfg(target_arch = "aarch64")]
fn host_target_arch() -> TargetArch {
    TargetArch::aarch64
}

/// `hyperion_trust_boundary::SpawnGrant`'s real, distinct IPC-rights dimension's own seccomp
/// half: exactly the four syscalls a real `std::os::unix::net::UnixDatagram` bind/`send_to`/
/// `recv_from` round trip actually makes on this target -- confirmed directly via `strace`
/// against a real minimal Rust binary performing that exact round trip (this crate's own
/// established rigor for every syscall list here, not guessed), rather than a broader "everything
/// socket-related" allowlist. `connect`/`listen`/`accept` are deliberately absent:
/// `hyperion_ipc::transport::Endpoint` is connectionless (`UnixDatagram`), and never calls them.
fn ipc_allowed_syscalls() -> Vec<i64> {
    vec![
        libc::SYS_socket,
        libc::SYS_bind,
        libc::SYS_sendto,
        libc::SYS_recvfrom,
    ]
}

/// Installs the baseline default-deny seccomp-bpf filter on the calling process: every syscall
/// in [`baseline_allowed_syscalls`] is allowed unconditionally, everything else returns `EPERM`.
/// `allow_ipc` additionally allows [`ipc_allowed_syscalls`] -- `hyperion_trust_boundary::SpawnGrant`'s
/// own real, distinct IPC-rights dimension; see [`apply_landlock`]'s own doc comment for the
/// matching real Landlock half that actually scopes *where* those syscalls may create a socket.
pub fn apply_seccomp(allow_ipc: bool) -> Result<(), EnforcementError> {
    let mut allowed = baseline_allowed_syscalls();
    if allow_ipc {
        allowed.extend(ipc_allowed_syscalls());
    }
    let rules = allowed
        .into_iter()
        .map(|syscall_nr| (syscall_nr, vec![]))
        .collect();

    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Errno(libc::EPERM as u32),
        SeccompAction::Allow,
        host_target_arch(),
    )?;
    let bpf_program: BpfProgram = filter.try_into()?;
    seccompiler::apply_filter(&bpf_program)?;

    Ok(())
}
