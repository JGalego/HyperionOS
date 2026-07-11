//! Proves M2's exit criteria for real: a capability token gates real filesystem access and a
//! real syscall surface on a real, separate Linux process (`sandbox_enforces_scoped_filesystem_and_denies_unlisted_syscalls`),
//! and revoking it has a real effect -- the process stops existing, not just a struct flag
//! flipping (`revoking_a_token_kills_the_real_process`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use hyperion_trust_boundary::{spawn, RightsMask, SpawnGrant, TrustBoundaryId, TrustDepth};

/// The default `CARGO_BIN_EXE_probe` binary is dynamically linked against the host's glibc,
/// which needs to *read* (not just execute) the dynamic linker and libc `.so` files from
/// `/lib`/`/usr/lib` as part of `execve()` itself -- outside any `fs_scope` a test grants, so
/// exec would fail under a real Landlock ReadFile restriction before the sandboxed program ever
/// got to run. A statically linked musl build needs nothing outside its own scope to start, the
/// same reasoning `hyperion-init` already applies. Built once per test run (`cargo build` is a
/// fast no-op on the second call), not committed, matching the rest of this workspace's
/// reproducible-build-not-vendored-binary convention.
fn probe_bin() -> PathBuf {
    let target = "x86_64-unknown-linux-musl";
    let status = Command::new("cargo")
        .args([
            "build",
            "--target",
            target,
            "--bin",
            "probe",
            "-p",
            "hyperion-trust-boundary",
        ])
        .status()
        .expect("run cargo build for the musl probe binary");
    assert!(status.success(), "building the musl probe binary failed");

    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/hyperion-trust-boundary has a workspace root two levels up")
        .to_path_buf();
    workspace_root
        .join("target")
        .join(target)
        .join("debug")
        .join("probe")
}

/// The probe writes its results file asynchronously after exec; poll briefly rather than
/// asserting on a fixed sleep, so the test isn't flaky under slow/loaded CI runners.
fn wait_for_nonempty_file(path: &Path, timeout: Duration) -> Option<String> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(contents) = std::fs::read_to_string(path) {
            if !contents.is_empty() {
                return Some(contents);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

#[test]
fn sandbox_enforces_scoped_filesystem_and_denies_unlisted_syscalls() {
    let allowed = tempfile::tempdir().expect("create allowed tempdir");
    let outside = tempfile::tempdir().expect("create outside tempdir");
    let outside_secret = outside.path().join("secret.txt");
    std::fs::write(&outside_secret, b"top secret").expect("write outside file");

    let mut monitor = hyperion_trust_boundary::CapabilityMonitor::new();
    let token = monitor.mint_root(
        RightsMask::READ | RightsMask::WRITE,
        TrustBoundaryId(1),
        None,
    );
    let grant = SpawnGrant {
        token,
        depth: TrustDepth::Process,
        fs_scope: allowed.path().to_path_buf(),
    };

    let mut command = Command::new(probe_bin());
    command
        .arg("sandbox-check")
        .env("PROBE_ALLOWED_DIR", allowed.path())
        .env("PROBE_OUTSIDE_PATH", &outside_secret);

    let _boundary = spawn(&grant, command).expect("spawn probe under real enforcement");

    let results_path = allowed.path().join("results.txt");
    let results = wait_for_nonempty_file(&results_path, Duration::from_secs(5))
        .unwrap_or_else(|| panic!("probe did not write {results_path:?} within timeout"));

    for expected in [
        "WRITE_IN_SCOPE: PASS",
        "READ_IN_SCOPE: PASS",
        "READ_OUT_OF_SCOPE: PASS",
        "SYSCALL_DENIED: PASS",
    ] {
        assert!(
            results.contains(expected),
            "expected {expected:?} in probe results, got:\n{results}"
        );
    }
}

#[test]
fn revoking_a_token_kills_the_real_process() {
    let allowed = tempfile::tempdir().expect("create allowed tempdir");

    let mut monitor = hyperion_trust_boundary::CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::READ, TrustBoundaryId(2), None);
    let grant = SpawnGrant {
        token,
        depth: TrustDepth::Process,
        fs_scope: allowed.path().to_path_buf(),
    };

    let mut command = Command::new(probe_bin());
    command.arg("sleep");
    let boundary = spawn(&grant, command).expect("spawn probe under real enforcement");

    // Give it a moment to actually reach the sleep loop before checking liveness.
    let start = Instant::now();
    while !boundary.is_alive() && start.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        boundary.is_alive(),
        "process should be alive before revocation"
    );

    let pid = boundary.pid();
    let _receipt = boundary.revoke(&mut monitor);

    // A raw kill(pid, 0) after revoke() (which already killed and reaped the child) must
    // report ESRCH ("no such process") -- the process doesn't merely look inaccessible, it is
    // gone.
    let rc = unsafe { libc::kill(pid, 0) };
    assert_eq!(rc, -1, "process should no longer exist after revocation");
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}
