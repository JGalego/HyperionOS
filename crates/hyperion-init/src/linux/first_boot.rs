//! This crate's own previously-unnamed gap: nothing anywhere in `hyperion-init` ever recognized
//! "this is this specific machine's actual first boot" as a first-class event of its own.
//! `hyperion-console::ConsoleSession::open` already has its own, lower-layer "first run" concepts
//! (`Keystore::open_or_create` minting a device identity, `KnowledgeGraph::seed_if_empty` seeding
//! a starter dataset) — both correctly scoped to the console's own data model, not this crate's
//! concern. What was missing is the one thing only PID 1 itself can know: whether *this real,
//! persistent M6 data partition* has ever been seen by `hyperion-init` before, at all.
//!
//! [`run_first_boot_hook`] is deliberately narrow: a marker file on the real, persistent data
//! partition, and a one-time welcome banner distinct from [`super::print_banner`]'s own per-boot
//! banner. Never called against the ephemeral tmpfs fallback (`CONSOLE_FALLBACK_DATA_DIR`) — that
//! directory is wiped every boot, so a marker written there would never survive to be found on
//! the next boot, making every boot look like a fresh install. Best-effort throughout (a failure
//! to read or write the marker degrades to "assume already provisioned," never blocks the boot
//! this returns into), matching this crate's own established "log, don't panic" shape for every
//! other probe (`storage_probe::mount_data_partition`, `hardware_fit::run_hardware_fit_probe`).

use std::path::Path;

const FIRST_BOOT_MARKER: &str = ".hyperion-first-boot-complete";

/// Runs once per real call site (see this module's own doc comment on why that's only ever the
/// real, mounted M6 partition): prints a one-time welcome banner and writes
/// [`FIRST_BOOT_MARKER`] the first time this exact `data_dir` is seen with no marker already on
/// it; silently does nothing on every subsequent boot once that marker exists.
pub fn run_first_boot_hook(data_dir: &Path) {
    let marker = data_dir.join(FIRST_BOOT_MARKER);
    if marker.exists() {
        return;
    }

    println!(
        "[hyperion-init] ============================================================\n\
         [hyperion-init] Welcome -- this is a fresh Hyperion install.\n\
         [hyperion-init] This machine's own real device identity and Knowledge Graph will be\n\
         [hyperion-init] created the first time the console starts.\n\
         [hyperion-init] ============================================================"
    );

    if let Err(e) = std::fs::write(&marker, b"") {
        eprintln!(
            "[hyperion-init] warning: couldn't write the real first-boot marker at {marker:?}: \
             {e} -- this welcome banner may print again on the next boot"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_genuinely_fresh_data_dir_gets_a_real_marker_written() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join(FIRST_BOOT_MARKER);
        assert!(!marker.exists());

        run_first_boot_hook(dir.path());

        assert!(
            marker.exists(),
            "the real first-boot marker must exist after the first real call"
        );
    }

    #[test]
    fn a_data_dir_with_an_existing_marker_is_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join(FIRST_BOOT_MARKER);
        std::fs::write(&marker, b"already provisioned").unwrap();
        let before = std::fs::read(&marker).unwrap();

        run_first_boot_hook(dir.path());

        let after = std::fs::read(&marker).unwrap();
        assert_eq!(
            before, after,
            "an already-provisioned machine's marker must never be rewritten"
        );
    }

    #[test]
    fn calling_it_twice_only_ever_writes_the_marker_once() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join(FIRST_BOOT_MARKER);

        run_first_boot_hook(dir.path());
        let first_metadata = std::fs::metadata(&marker).unwrap().modified().unwrap();

        run_first_boot_hook(dir.path());
        let second_metadata = std::fs::metadata(&marker).unwrap().modified().unwrap();

        assert_eq!(
            first_metadata, second_metadata,
            "a second real call must be a genuine no-op, never rewriting the marker"
        );
    }
}
