use std::path::PathBuf;

use hyperion_capability::CapabilityToken;

/// A Trust Depth from docs/03-kernel-architecture.md's sandboxing spectrum, restricted to the
/// two depths that mean "spawn a real, separate Linux process":
///
/// - Depth 0 (in-process) spawns nothing -- it's a language-level boundary (WASM, Rust type
///   safety), out of scope for a *process* spawner by definition.
/// - Depth 3 (VM) means hardware virtualization with an IOMMU-isolated device model -- a real
///   hypervisor integration, not an incremental extension of this crate. Deliberately deferred
///   as its own future project, not attempted here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustDepth {
    /// Depth 1: MMU address space + per-process capability table, sharing the kernel ABI and
    /// scheduler with its parent. The default for drivers, Capabilities, and Agents. Gets
    /// seccomp + Landlock scoping; no namespace isolation.
    Process,
    /// Depth 2: adds namespace isolation (mount/net/uts/ipc) on top of Process's seccomp +
    /// Landlock scoping, for compatibility-layer Linux/Android apps.
    ///
    /// PID namespace isolation is deliberately not included: `unshare(CLONE_NEWPID)` only takes
    /// effect for children forked *after* the call, never the calling process itself, so making
    /// the spawned program actually run inside a fresh PID namespace needs a second fork with
    /// something acting as that namespace's PID 1 -- exactly the supervisor M5 builds for real.
    /// Adding a one-off reaper here would duplicate that work ahead of it existing for real.
    Container,
}

/// What a Trust Boundary is granted at spawn time: the capability token naming its rights, the
/// isolation depth, and the one filesystem path its Landlock ruleset scopes those rights to.
///
/// A single scoped path is a deliberate simplification, not a limitation of the underlying
/// mechanism: `hyperion_capability::CapabilityToken` names one `ObjectId`, and for this
/// milestone's proof (a real capability gating real filesystem access) that object is a
/// directory. Multiple simultaneous path grants per token are a real, later extension, not a
/// redesign, once a caller actually needs them.
#[derive(Debug, Clone)]
pub struct SpawnGrant {
    pub token: CapabilityToken,
    pub depth: TrustDepth,
    pub fs_scope: PathBuf,
    /// A real, distinct IPC-rights dimension -- `hyperion-supervisor`'s own previously-named gap
    /// ("would need allowlisting AF_UNIX socket syscalls and Landlock MakeSock rights, a real but
    /// separable extension"), closed here. `None` (every existing caller's default) grants no IPC
    /// rights at all -- `socket`/`bind`/`sendto`/`recvfrom` stay denied by the baseline seccomp
    /// filter exactly as before. `Some(rendezvous_path)` is the one specific socket path (e.g. a
    /// per-service `HYPERION_IPC_SOCK` convention) this boundary may really `bind()` a real
    /// `std::os::unix::net::UnixDatagram` at -- deliberately not folded into `fs_scope`/`RightsMask`
    /// (a service that can read/write its own working directory has no reason to also be able to
    /// create arbitrary sockets there, and vice versa), so a boundary can hold real filesystem
    /// rights, real IPC rights, both, or neither, independently.
    pub ipc_rendezvous: Option<PathBuf>,
}
