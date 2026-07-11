//! Hyperion's PID 1 on the Linux-hosted MVP.
//!
//! Per [PRODUCTION_BOOT_PROMPT.md](../../../PRODUCTION_BOOT_PROMPT.md) M1, this replaces
//! Buildroot's stock BusyBox init to prove the custom-init boot path end to end: mount what's
//! needed, print a banner, bring up the real supervision tree. M5 replaced the original M1
//! placeholder (a single hardcoded supervised shell loop) with [`hyperion_supervisor::Supervisor`]:
//! every Phase 2-10 subsystem this image ships runs as a real, capability-scoped, supervised
//! process, restarted with a fresh capability grant if it crashes, alongside a carryover debug
//! shell folded into the same supervision tree (see [`linux::run`]'s own docs for why the shell
//! specifically isn't capability-scoped the same way).

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
