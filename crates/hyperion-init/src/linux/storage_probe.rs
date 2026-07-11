//! Mounts Hyperion's real, dedicated persistent-storage partition, per
//! [PRODUCTION_BOOT_PROMPT.md](../../../PRODUCTION_BOOT_PROMPT.md) M6 -- `hyperion-storage`'s
//! WAL-backed engine pointed at a real block device instead of a host tempfile, per the roadmap's
//! own reuse-map entry ("WAL format, replay/recovery logic" reused as-is; only *where* the WAL
//! file lives changes). `hyperion-storage`'s own `Wal` already relies on ordinary regular-file
//! append semantics (`O_APPEND`), which only behave the way a growing log needs on a real
//! filesystem, not a raw block device (`O_APPEND` on a block special file seeks to the device's
//! total reported capacity, not "the end of what's been written so far") -- so this mounts a real
//! filesystem on the dedicated device rather than pointing the WAL at a raw device node, matching
//! the roadmap's own "block device *or partition*" wording and the reuse-map's "reused as-is"
//! promise for the WAL itself.
//!
//! The dedicated device (a second virtio-blk drive distinct from the boot disk, pre-formatted
//! ext4 at image-build time -- this minimal rootfs has no `mkfs.ext4` to format one at runtime,
//! and shouldn't need one: a fresh data volume is provisioned once, not reformatted on every
//! boot) is entirely optional at boot: a normal boot with only the one boot disk attached (real
//! hardware not yet given a second drive, or `boot/scripts/run-qemu.sh`'s default single-disk
//! invocation) simply has nothing at `/dev/vdb`, so [`mount_data_partition`] finds nothing to do
//! and returns `None` -- not a boot failure, the same best-effort shape as this crate's own
//! cgroup bootstrap.
//!
//! [`run_crash_consistency_probe`] is this milestone's real power-loss verification, self-driven
//! entirely from what it finds already on the mounted partition: empty (a fresh data disk) means
//! start a real WAL write loop; non-empty (rebooted after `boot/scripts/storage-crash-test.sh`
//! hard-killed `qemu` mid-loop) means verify the recovered value is a real, uncorrupted, in-range
//! version of the same object -- never garbage, never silently partial -- and report the result
//! to the serial console the test script scrapes. This only ever does anything when the dedicated
//! device is actually present, so it's inert on every other boot.

use std::ffi::CString;
use std::path::{Path, PathBuf};

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_storage::{ObjectId, StorageEngine};

const DATA_DEVICE: &str = "/dev/vdb";
const DATA_MOUNTPOINT: &str = "/var/lib/hyperion/data";
const CRASH_TEST_WAL_PATH: &str = "/var/lib/hyperion/data/crash_test.wal";
/// Deliberately large: this loop is meant to still be mid-flight whenever
/// `boot/scripts/storage-crash-test.sh` hard-kills the VM after its own short, fixed delay, not to
/// ever run to completion during that test. A real workload would never write this way; this
/// exists purely to give the crash test a wide, reliable window to interrupt.
const CRASH_TEST_WRITE_COUNT: u64 = 200_000;
const CRASH_TEST_PROGRESS_STRIDE: u64 = 200;

/// Mounts the real, dedicated data partition if a second block device is present. Returns the
/// mountpoint on success so a caller can point real storage at it; `None` (logged, not fatal) if
/// there's no dedicated device at all, or mounting it failed for some other reason.
pub fn mount_data_partition() -> Option<PathBuf> {
    if !Path::new(DATA_DEVICE).exists() {
        return None;
    }

    if let Err(e) = std::fs::create_dir_all(DATA_MOUNTPOINT) {
        eprintln!("[hyperion-init] warning: mkdir {DATA_MOUNTPOINT} failed: {e}");
        return None;
    }

    let source = CString::new(DATA_DEVICE).expect("no interior NUL");
    let target = CString::new(DATA_MOUNTPOINT).expect("no interior NUL");
    let fstype = CString::new("ext4").expect("no interior NUL");
    // SAFETY: source/target/fstype are valid, NUL-terminated C strings kept alive for this call;
    // `data` is null, which ext4 accepts.
    let rc = unsafe {
        libc::mount(
            source.as_ptr(),
            target.as_ptr(),
            fstype.as_ptr(),
            0,
            std::ptr::null(),
        )
    };

    if rc == 0 {
        println!("[hyperion-init] mounted real data partition {DATA_DEVICE} at {DATA_MOUNTPOINT}");
        Some(PathBuf::from(DATA_MOUNTPOINT))
    } else {
        eprintln!(
            "[hyperion-init] warning: failed to mount {DATA_DEVICE} at {DATA_MOUNTPOINT}: {}",
            std::io::Error::last_os_error()
        );
        None
    }
}

/// M6's real power-loss re-validation of `hyperion-storage`'s existing crash-consistency
/// guarantee (docs/28 §Testing Strategy: "replay always converges to a state consistent with the
/// last durably committed WAL record — never partial"), now against a real mounted partition on a
/// real (virtio) block device instead of a host tempfile. Only ever runs at all when
/// [`mount_data_partition`] found a real dedicated device to mount -- inert on every other boot.
pub fn run_crash_consistency_probe(data_dir: &Path) {
    let wal_path = data_dir.join("crash_test.wal");
    debug_assert_eq!(wal_path, Path::new(CRASH_TEST_WAL_PATH));

    let engine = match StorageEngine::open(&wal_path) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[hyperion-init] CRASH_TEST: failed to open the real storage engine: {e}");
            return;
        }
    };

    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(0), None);
    let object_id = ObjectId(0);

    match engine.current_version(object_id) {
        None => run_write_loop(&engine, &monitor, &token, object_id),
        Some(_) => verify_replay(&engine, &monitor, &token, object_id),
    }
}

fn run_write_loop(
    engine: &StorageEngine,
    monitor: &CapabilityMonitor,
    token: &hyperion_capability::CapabilityToken,
    object_id: ObjectId,
) {
    println!("[hyperion-init] CRASH_TEST: fresh data partition, starting real WAL write loop");
    let mut expected_version = None;
    for i in 1..=CRASH_TEST_WRITE_COUNT {
        let result = engine.put_object(
            monitor,
            token,
            Some(object_id),
            expected_version,
            serde_json::json!({ "seq": i }),
        );
        match result {
            Ok((_, new_version)) => expected_version = Some(new_version),
            Err(e) => {
                eprintln!("[hyperion-init] CRASH_TEST: write error at seq={i}: {e}");
                return;
            }
        }
        if i % CRASH_TEST_PROGRESS_STRIDE == 0 {
            println!("[hyperion-init] CRASH_TEST: wrote {i}");
        }
    }
    println!("[hyperion-init] CRASH_TEST: wrote all {CRASH_TEST_WRITE_COUNT} records");
}

/// A genuinely broken replay (a corrupted or partially-applied WAL record slipping through)
/// would almost certainly not coincidentally decode into another small, in-range integer here --
/// this is a real, non-vacuous check of the same "never partial" property
/// `tests/wal_recovery.rs`'s `recovery_tolerates_a_torn_trailing_record...` test already proves
/// against a host tempfile, now against whatever this real partition actually contains after a
/// real hard kill.
fn verify_replay(
    engine: &StorageEngine,
    monitor: &CapabilityMonitor,
    token: &hyperion_capability::CapabilityToken,
    object_id: ObjectId,
) {
    match engine.get_object(monitor, token, object_id, None) {
        Ok(value) => {
            let seq = value.get("seq").and_then(|v| v.as_u64());
            match seq {
                Some(k) if (1..=CRASH_TEST_WRITE_COUNT).contains(&k) => {
                    println!(
                        "[hyperion-init] CRASH_TEST: replay result: consistent (recovered seq={k})"
                    );
                }
                _ => {
                    println!(
                        "[hyperion-init] CRASH_TEST: replay result: INCONSISTENT (bad value: \
                         {value:?})"
                    );
                }
            }
        }
        Err(e) => {
            println!("[hyperion-init] CRASH_TEST: replay result: INCONSISTENT (get_object: {e})");
        }
    }
}
