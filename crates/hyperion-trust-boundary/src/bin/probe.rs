//! A companion binary for hyperion-trust-boundary's integration tests. Exec'd inside a spawned
//! Trust Boundary, it performs a few file/syscall operations and writes one PASS/FAIL line per
//! check to a results file *inside* its own sandboxed directory (not stdout: the test harness
//! runs tests concurrently in one process, so redirecting the test binary's own stdout fd to
//! capture a child's output would race with unrelated tests), so the parent can confirm
//! Landlock/seccomp actually restrict this process rather than merely trusting they were
//! installed.

use std::env;
use std::fs;
use std::io::ErrorKind;

fn main() {
    match env::args().nth(1).as_deref() {
        Some("sandbox-check") => sandbox_check(),
        Some("sleep") => sleep_forever(),
        _ => {
            eprintln!("usage: probe <sandbox-check|sleep>");
            std::process::exit(2);
        }
    }
}

fn sandbox_check() {
    let allowed_dir = env::var("PROBE_ALLOWED_DIR").expect("PROBE_ALLOWED_DIR not set");
    let outside_path = env::var("PROBE_OUTSIDE_PATH").expect("PROBE_OUTSIDE_PATH not set");
    let mut results = String::new();

    let in_scope_file = format!("{allowed_dir}/probe.txt");
    match fs::write(&in_scope_file, b"hello") {
        Ok(()) => results.push_str("WRITE_IN_SCOPE: PASS\n"),
        Err(e) => results.push_str(&format!("WRITE_IN_SCOPE: FAIL ({e})\n")),
    }
    match fs::read_to_string(&in_scope_file) {
        Ok(s) if s == "hello" => results.push_str("READ_IN_SCOPE: PASS\n"),
        Ok(s) => results.push_str(&format!("READ_IN_SCOPE: FAIL (unexpected content {s:?})\n")),
        Err(e) => results.push_str(&format!("READ_IN_SCOPE: FAIL ({e})\n")),
    }

    match fs::read_to_string(&outside_path) {
        Ok(_) => results.push_str("READ_OUT_OF_SCOPE: FAIL (unexpectedly succeeded)\n"),
        Err(e) if e.kind() == ErrorKind::PermissionDenied => {
            results.push_str(&format!("READ_OUT_OF_SCOPE: PASS (denied: {e})\n"));
        }
        Err(e) => results.push_str(&format!(
            "READ_OUT_OF_SCOPE: FAIL (wrong error kind: {e})\n"
        )),
    }

    // SAFETY: socket() is a deliberately-unlisted syscall in the baseline seccomp allowlist;
    // any return value (success or failure) is safe to inspect and, on success, to close.
    let rc = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    if rc < 0 {
        results.push_str(&format!(
            "SYSCALL_DENIED: PASS ({})\n",
            std::io::Error::last_os_error()
        ));
    } else {
        results.push_str("SYSCALL_DENIED: FAIL (socket() unexpectedly succeeded)\n");
        // SAFETY: rc is the valid fd just returned by the successful socket() call above.
        unsafe {
            libc::close(rc);
        }
    }

    // Best-effort: if even this write fails, the test observes an empty/missing results file,
    // which itself fails the test loudly rather than hanging.
    let _ = fs::write(format!("{allowed_dir}/results.txt"), results);
}

fn sleep_forever() -> ! {
    println!("SLEEPING");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
