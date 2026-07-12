//! Real, kernel-enforced Trust Boundaries.
//!
//! Rehosts `hyperion-capability`'s token/derivation/revocation-graph algorithm (reused as-is;
//! see that crate) so that minting, deriving, and revoking a [`CapabilityToken`] has a real
//! effect on a real Linux process, per
//! [PRODUCTION_BOOT_PROMPT.md](../../../PRODUCTION_BOOT_PROMPT.md) M2 and
//! [docs/03-kernel-architecture.md](../../../docs/03-kernel-architecture.md)'s sandboxing
//! spectrum. [`spawn`] forks and applies the real enforcement (namespaces, Landlock, seccomp)
//! that a [`SpawnGrant`]'s `RightsMask` and [`TrustDepth`] call for; [`SpawnedBoundary::revoke`]
//! kills the process and revokes its token, so "revoked" means the same thing to the capability
//! algorithm and to the operating system.
//!
//! Every mechanism here (user namespaces, seccomp-bpf, Landlock) is deliberately used
//! unprivileged: none of the three requires host root, so this crate never assumes it (the
//! roadmap's "privileged root-owned Capability Monitor daemon" framing describes a traditional
//! multi-user setup; a single unprivileged process gaining "root" only within its own fresh user
//! namespace is sufficient to enforce every depth this crate implements, and is a smaller trust
//! base besides). See [`TrustDepth`] for exactly which depths are implemented and why depth 3
//! (VM) is not.

// `enforcement`/`spawn` are Linux-only: their whole job is real namespaces/Landlock/seccomp-bpf,
// kernel mechanisms with no macOS equivalent (and `landlock`/`seccompiler`, their backing crates,
// don't even compile there -- see Cargo.toml's own comment). `errors`/`types` are plain data with
// no OS-specific dependency and stay available everywhere so a cross-platform caller (or `cargo
// build --workspace --all-targets` on macOS CI) can still see this crate's shape.
#[cfg(target_os = "linux")]
mod enforcement;
mod errors;
#[cfg(target_os = "linux")]
mod spawn;
mod types;

#[cfg(target_os = "linux")]
pub use enforcement::fs_access_for_rights;
pub use errors::EnforcementError;
#[cfg(target_os = "linux")]
pub use spawn::{spawn, SpawnedBoundary};
pub use types::{SpawnGrant, TrustDepth};

// Re-exported so callers don't need a direct `hyperion-capability` dependency just to build a
// `SpawnGrant` or call `SpawnedBoundary::revoke`.
pub use hyperion_capability::{
    CapabilityMonitor, CapabilityToken, RevocationReceipt, RightsMask, TrustBoundaryId,
};
