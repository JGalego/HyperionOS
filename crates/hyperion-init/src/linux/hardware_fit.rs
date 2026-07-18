//! A real, physical hardware-fit probe -- this crate's own previously-unnamed gap: nothing
//! anywhere in this workspace ever checks the *actual machine's* real available RAM or real free
//! disk space before booting the rest of the supervision tree. `hyperion-ai-runtime`'s own
//! `ResidencyManager` (see `residency.rs` there) tracks in-process memory bookkeeping against a
//! caller-supplied budget (`total_capacity_mb`), but every real caller in this workspace hardcodes
//! that number as a plain literal -- none of them ever asks the real host how much RAM it
//! actually has. This module is the first real producer of that number: run once, here, before
//! anything else starts, using nothing but this crate's own already-established real, minimal
//! tools (`libc` syscalls, raw `/proc` reads) -- no new dependency, matching `storage_probe`'s own
//! restraint.
//!
//! Deliberately a warning, never a boot-blocking failure: a real machine below these thresholds
//! can still boot and run Hyperion's own console on [`crate::linux::MockBackend`]-class inference
//! (or a smaller real model than the default) -- refusing to boot at all over a memory/disk
//! shortfall would leave a person with a genuinely less useful failure mode (no system at all)
//! than a real, honest warning plus a system that still runs, degraded. Matches
//! `storage_probe::mount_data_partition`'s own "log, don't panic" shape for a missing resource.
//!
//! These thresholds are deliberately modest, not docs/36's full production "small resident"
//! (1-3B-parameter, real NPU/GPU-class memory) tier -- they're sized for `hyperion-ai-runtime`'s
//! own real, already-boot-tested `CandleBackend::load()` default (a genuinely tiny 15M-parameter
//! checkpoint, ~61 MB on disk, comfortably under 200 MB resident) plus this console's own real
//! Knowledge Graph/keystore/secret-store overhead, not a claim about what any larger model needs.

use std::path::Path;

/// The minimum real, physically available RAM this probe considers enough to run the console
/// plus `hyperion-ai-runtime`'s own real, tiny default Candle checkpoint comfortably -- see this
/// module's own doc comment for why this is not a docs/36 production-tier figure.
const MIN_AVAILABLE_RAM_MB: u64 = 512;
/// The minimum real, physically free disk space on the real data partition (or wherever the
/// console's own data directory ends up living -- see `storage_probe`/`console_data_dir`) this
/// probe considers enough for a fresh Knowledge Graph WAL, device keystore, and encrypted
/// secret store to have real room to grow, not a hard ceiling on how large those can ever become.
const MIN_FREE_DISK_MB: u64 = 256;

/// The real result of one hardware-fit probe -- `None` fields mean this probe couldn't determine
/// that real quantity at all (an unreadable `/proc/meminfo`, a `statvfs` failure), treated the
/// same permissive way as actually meeting the threshold: this probe only ever warns about a
/// real, confirmed shortfall, never about its own inability to check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HardwareFitReport {
    pub available_ram_mb: Option<u64>,
    pub free_disk_mb: Option<u64>,
}

impl HardwareFitReport {
    pub fn ram_meets_minimum(&self) -> bool {
        self.available_ram_mb
            .is_none_or(|mb| mb >= MIN_AVAILABLE_RAM_MB)
    }

    pub fn disk_meets_minimum(&self) -> bool {
        self.free_disk_mb.is_none_or(|mb| mb >= MIN_FREE_DISK_MB)
    }
}

/// Runs the real probe against `data_dir` (wherever the console's own real data directory will
/// live -- the mounted M6 data partition if one is attached, `/` otherwise) and prints a real,
/// honest warning for any real, confirmed shortfall -- never blocks the boot this function
/// returns into.
pub fn run_hardware_fit_probe(data_dir: &Path) -> HardwareFitReport {
    let report = HardwareFitReport {
        available_ram_mb: available_ram_mb(),
        free_disk_mb: free_disk_mb(data_dir),
    };

    match report.available_ram_mb {
        Some(mb) if !report.ram_meets_minimum() => eprintln!(
            "[hyperion-init] warning: only {mb} MB of RAM is really available (this build's own \
             minimum is {MIN_AVAILABLE_RAM_MB} MB) -- continuing, but real local inference may \
             fail or run in real degraded mode"
        ),
        Some(mb) => println!("[hyperion-init] hardware fit: {mb} MB of RAM really available"),
        None => eprintln!(
            "[hyperion-init] warning: couldn't determine real available RAM (unreadable \
             /proc/meminfo) -- continuing without this check"
        ),
    }

    match report.free_disk_mb {
        Some(mb) if !report.disk_meets_minimum() => eprintln!(
            "[hyperion-init] warning: only {mb} MB of disk is really free at {data_dir:?} (this \
             build's own minimum is {MIN_FREE_DISK_MB} MB) -- continuing, but the real Knowledge \
             Graph/keystore may run out of room soon"
        ),
        Some(mb) => {
            println!("[hyperion-init] hardware fit: {mb} MB of disk really free at {data_dir:?}")
        }
        None => eprintln!(
            "[hyperion-init] warning: couldn't determine real free disk space at {data_dir:?} \
             -- continuing without this check"
        ),
    }

    report
}

/// A real reading of `/proc/meminfo`'s own `MemAvailable` line -- the kernel's own real estimate
/// of memory available for a new process without swapping, not the cruder (and here, misleading)
/// `MemFree` figure that ignores real, reclaimable page/buffer cache.
fn available_ram_mb() -> Option<u64> {
    let contents = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in contents.lines() {
        if let Some(rest) = line.strip_prefix("MemAvailable:") {
            let kb: u64 = rest.trim().trim_end_matches("kB").trim().parse().ok()?;
            return Some(kb / 1024);
        }
    }
    None
}

/// A real `statvfs(2)` call against `path` -- the same real, physical "how much room is actually
/// left" a `df` invocation would report, not any in-process accounting.
fn free_disk_mb(path: &Path) -> Option<u64> {
    let c_path = std::ffi::CString::new(path.to_str()?).ok()?;
    // SAFETY: `stat` is a plain C struct with no invariants beyond being zero-initializable
    // before `statvfs` fills it in; `c_path` is a valid, NUL-terminated C string kept alive for
    // the duration of this call.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
    if rc != 0 {
        return None;
    }
    Some((stat.f_bavail * stat.f_frsize) / (1024 * 1024))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_with_no_data_meets_both_minimums_permissively() {
        let report = HardwareFitReport {
            available_ram_mb: None,
            free_disk_mb: None,
        };
        assert!(report.ram_meets_minimum());
        assert!(report.disk_meets_minimum());
    }

    #[test]
    fn a_report_below_either_real_minimum_fails_that_check_only() {
        let report = HardwareFitReport {
            available_ram_mb: Some(MIN_AVAILABLE_RAM_MB - 1),
            free_disk_mb: Some(MIN_FREE_DISK_MB),
        };
        assert!(!report.ram_meets_minimum());
        assert!(report.disk_meets_minimum());
    }

    #[test]
    fn a_report_at_or_above_both_real_minimums_meets_both() {
        let report = HardwareFitReport {
            available_ram_mb: Some(MIN_AVAILABLE_RAM_MB),
            free_disk_mb: Some(MIN_FREE_DISK_MB * 10),
        };
        assert!(report.ram_meets_minimum());
        assert!(report.disk_meets_minimum());
    }

    /// This probe's own two real syscalls, run for real against this test's own real host (not
    /// mocked) -- proving the raw `/proc/meminfo`/`statvfs` parsing actually works against
    /// whatever machine `cargo test` happens to run on, not just against synthetic strings.
    #[test]
    fn the_real_probes_return_real_plausible_numbers_on_this_real_host() {
        let ram = available_ram_mb();
        assert!(
            ram.is_some_and(|mb| mb > 0),
            "a real Linux host always has /proc/meminfo with a real, positive MemAvailable"
        );

        let disk = free_disk_mb(Path::new("/"));
        assert!(
            disk.is_some_and(|mb| mb > 0),
            "a real Linux host always has real, positive free space on /"
        );
    }
}
