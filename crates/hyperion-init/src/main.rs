//! Hyperion's PID 1 on the Linux-hosted MVP.
//!
//! Per [PRODUCTION_BOOT_PROMPT.md](../../../PRODUCTION_BOOT_PROMPT.md) M1, this replaces
//! Buildroot's stock BusyBox init to prove the custom-init boot path end to end: mount what's
//! needed, print a banner, supervise a shell. It is deliberately trivial -- M5 replaces
//! [`linux::run`] with the real Erlang/OTP-style supervision tree that starts the Capability
//! Monitor (M2), the IPC bus (M3), the scheduler enforcement daemon (M4), and every Phase 2-10
//! subsystem as a capability-scoped supervised process. Until then, this crate's only job is to
//! prove that *something other than BusyBox's init* can be PID 1 and bring the system up cleanly.

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
fn main() {
    linux::run();
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!(
        "hyperion-init is a Linux PID 1 replacement; there is nothing to run on this platform."
    );
}
