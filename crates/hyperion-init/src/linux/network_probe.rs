//! Real guest network interface bring-up at boot -- named as a deferred gap in
//! docs/998-roadmap.md M10 ("the guest's network interface is never brought up at real
//! boot time"), closed here. The kernel itself completes a real DHCP handshake before any
//! userspace process (including this one) ever runs, driven by the `ip=dhcp` kernel cmdline
//! parameter and `CONFIG_IP_PNP`/`CONFIG_IP_PNP_DHCP` -- no new userspace DHCP client package
//! needed, and no dependency on which DHCP server answered (QEMU's own SLIRP proxy for the dev
//! loop, or a real router on real hardware): the kernel populates `/proc/net/pnp` with whatever
//! the real lease actually contained, and this just reads that back.
//!
//! `/proc/net/pnp`'s own format (from the kernel's `net/ipv4/ipconfig.c`) is already
//! `/etc/resolv.conf`-compatible line for line -- `nameserver <ip>` and `domain <name>` lines,
//! plus a leading `#PROTO: ...` comment this module strips. Inert (logs a note, does nothing
//! else) if the interface never came up at all (no real DHCP server reachable, or `ip=dhcp`
//! wasn't passed) -- never blocks boot on a missing network, matching every other real-but-
//! optional mechanism in this crate (the data partition, cgroups).

use std::path::Path;

const PNP_PATH: &str = "/proc/net/pnp";
const RESOLV_CONF_PATH: &str = "/etc/resolv.conf";

/// Reads the kernel's own real DHCP lease info from `/proc/net/pnp` (populated by
/// `CONFIG_IP_PNP_DHCP` before this process ever started) and writes it as a real
/// `/etc/resolv.conf`. Does nothing (logged, not fatal) if `/proc/net/pnp` doesn't exist (no
/// `ip=dhcp`/`CONFIG_IP_PNP` in this kernel build) or contains no real `nameserver`/`domain`
/// lines (interface never got a real lease -- e.g. no DHCP server reachable).
pub fn write_resolv_conf_from_kernel_dhcp() {
    let raw = match std::fs::read_to_string(PNP_PATH) {
        Ok(s) => s,
        Err(e) => {
            println!(
                "[hyperion-init] NETWORK: no real kernel DHCP info at {PNP_PATH} ({e}) -- \
                 skipping /etc/resolv.conf generation"
            );
            return;
        }
    };
    println!("[hyperion-init] NETWORK: real {PNP_PATH} contents:\n{raw}");

    let lines: Vec<&str> = raw
        .lines()
        .filter(|line| line.starts_with("nameserver") || line.starts_with("domain"))
        .collect();

    if lines.is_empty() {
        println!(
            "[hyperion-init] NETWORK: {PNP_PATH} has no real nameserver/domain lines -- the \
             interface never got a real DHCP lease; skipping /etc/resolv.conf generation"
        );
        return;
    }

    let mut contents = lines.join("\n");
    contents.push('\n');

    match std::fs::write(Path::new(RESOLV_CONF_PATH), &contents) {
        Ok(()) => println!(
            "[hyperion-init] NETWORK: wrote real {RESOLV_CONF_PATH} from the kernel's own real \
             DHCP lease:\n{contents}"
        ),
        Err(e) => {
            println!("[hyperion-init] NETWORK: failed to write {RESOLV_CONF_PATH}: {e}")
        }
    }
}
