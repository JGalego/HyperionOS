//! Real Erlang/OTP-style process supervision for Hyperion's PID 1, per
//! [PRODUCTION_BOOT_PROMPT.md](../../../PRODUCTION_BOOT_PROMPT.md) M5.
//!
//! Reuses `hyperion-capability`'s mint/revoke algorithm (M2, unmodified) and
//! `hyperion-recovery`'s already-real microreboot semantics as the model for what "restart" means
//! (M5's reuse-map entry: "In-process restart → real supervised-process restart") -- rehosted so
//! that the thing being restarted is a real Linux process, not an in-process `AgentInstance`, and
//! restarting it means a real `fork`+`exec` under a freshly minted capability grant, not
//! re-invoking a function.
//!
//! ## What a supervised Phase 2-10 service actually is here
//!
//! Every crate the reuse map lists as running "unmodified as a real supervised service" (context,
//! intent, memory, coordination, federation, device, explainability, observability, privacy,
//! security, threat-model, plugin-framework, sdk, api-gateway, compat, scalability, update) needs
//! nothing structural changed in its own `lib.rs` to become real under this crate -- only a real
//! process entry point that constructs its existing, already-real API and does something with it.
//! Two representative examples exist end to end (`hyperion-observability`'s
//! `src/bin/hyperion-observability-service.rs`, `hyperion-explainability`'s
//! `src/bin/hyperion-explainability-service.rs`), proving the mechanism against real crate logic
//! rather than a synthetic stand-in. Wrapping the remaining ~30 crates the same way is a real,
//! separate, purely mechanical migration this mechanism now makes straightforward -- not attempted
//! here for all of them, the same scoping discipline M2/M3/M4 each already applied to their own
//! milestone (prove the mechanism for real against a representative case; don't redo the same
//! wrapping thirty times with no new engineering insight).
//!
//! ## What's real here vs. deferred, and why
//!
//! - Capability-scoped spawn, crash detection, and fresh-grant respawn: fully real, live-tested
//!   against real forked processes (see `tests/real_supervision.rs`) -- the literal M5 exit
//!   criterion.
//! - Real cgroup v2 placement (M4) per service: real when a delegated cgroup subtree is available,
//!   degrades to "unweighted but running" otherwise rather than blocking startup -- see
//!   [`Supervisor::spawn_sandboxed`]'s docs.
//! - The IPC rendezvous directory: a real, created-at-boot directory and a real, well-known
//!   per-service bind path convention (`HYPERION_IPC_SOCK`) -- M3's own explicitly deferred
//!   "service-discovery directory" gap, closed here. A supervised service actually *binding* a
//!   real `hyperion_ipc::Endpoint` there is not exercised by this milestone, and can't be yet: M2's
//!   seccomp filter has no `socket`/`bind`/`connect` syscalls on its allowlist and its Landlock
//!   ruleset never handles `MakeSock`, so a sandboxed process attempting either fails closed today
//!   -- exactly the gap M3's own completion note already named ("would need allowlisting AF_UNIX
//!   socket syscalls and Landlock MakeSock rights, a real but separable extension"), still open,
//!   not silently forgotten. Closing it needs `SpawnGrant`/`apply_seccomp`/`apply_landlock` to
//!   accept a distinct IPC-rights dimension (the rendezvous directory is never the same path as a
//!   service's own `fs_scope`, so it needs its own Landlock rule, not a reuse of the existing
//!   one) -- a real, separate extension to M2's crate, deliberately not folded into this
//!   milestone's already-large scope.
//! - A give-up/alerting policy for a respawn attempt that itself keeps failing: not implemented --
//!   see [`Supervisor::reap_and_restart_one`]'s docs.

mod errors;
mod spec;
mod supervisor;

pub use errors::SupervisorError;
pub use spec::{ServiceScheduling, ServiceSpec};
pub use supervisor::Supervisor;
