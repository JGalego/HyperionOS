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
use std::os::unix::net::UnixDatagram;

fn main() {
    match env::args().nth(1).as_deref() {
        Some("sandbox-check") => sandbox_check(),
        Some("sleep") => sleep_forever(),
        Some("ipc-check") => ipc_check(),
        _ => {
            eprintln!("usage: probe <sandbox-check|sleep|ipc-check>");
            std::process::exit(2);
        }
    }
}

/// Proves `hyperion_trust_boundary::SpawnGrant::ipc_rendezvous`'s real effect from inside a real
/// sandboxed process: binding a real `UnixDatagram` at the granted rendezvous path, and a real
/// send/receive round trip through it, both really succeed under real Landlock/seccomp
/// enforcement -- and binding a second socket *outside* the granted rendezvous directory is
/// really denied, proving the real Landlock `MakeSock` rule is genuinely scoped to that
/// directory, not merely "sockets work everywhere once IPC is granted."
fn ipc_check() {
    let rendezvous = env::var("PROBE_IPC_RENDEZVOUS").expect("PROBE_IPC_RENDEZVOUS not set");
    let outside_dir = env::var("PROBE_OUTSIDE_DIR").expect("PROBE_OUTSIDE_DIR not set");
    let results_dir = env::var("PROBE_ALLOWED_DIR").expect("PROBE_ALLOWED_DIR not set");
    let mut results = String::new();

    match UnixDatagram::bind(&rendezvous) {
        Ok(socket) => {
            results.push_str("BIND_RENDEZVOUS: PASS\n");
            let peer = format!("{rendezvous}.peer");
            match UnixDatagram::bind(&peer) {
                Ok(peer_socket) => {
                    match socket.send_to(b"hello", &peer) {
                        Ok(_) => results.push_str("SEND_TO_PEER: PASS\n"),
                        Err(e) => results.push_str(&format!("SEND_TO_PEER: FAIL ({e})\n")),
                    }
                    let mut buf = [0u8; 16];
                    match peer_socket.recv_from(&mut buf) {
                        Ok((n, _)) if &buf[..n] == b"hello" => {
                            results.push_str("RECV_FROM_PEER: PASS\n")
                        }
                        Ok((n, _)) => results.push_str(&format!(
                            "RECV_FROM_PEER: FAIL (unexpected content {:?})\n",
                            &buf[..n]
                        )),
                        Err(e) => results.push_str(&format!("RECV_FROM_PEER: FAIL ({e})\n")),
                    }
                }
                Err(e) => results.push_str(&format!("SEND_TO_PEER: FAIL (peer bind: {e})\n")),
            }
        }
        Err(e) => results.push_str(&format!("BIND_RENDEZVOUS: FAIL ({e})\n")),
    }

    let outside_path = format!("{outside_dir}/outside.sock");
    match UnixDatagram::bind(&outside_path) {
        Ok(_) => results.push_str("BIND_OUTSIDE_SCOPE: FAIL (unexpectedly succeeded)\n"),
        Err(e) if e.kind() == ErrorKind::PermissionDenied => {
            results.push_str(&format!("BIND_OUTSIDE_SCOPE: PASS (denied: {e})\n"));
        }
        Err(e) => results.push_str(&format!(
            "BIND_OUTSIDE_SCOPE: FAIL (wrong error kind: {e})\n"
        )),
    }

    let _ = fs::write(format!("{results_dir}/results.txt"), results);
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
