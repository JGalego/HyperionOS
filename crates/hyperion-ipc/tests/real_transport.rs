//! Proves M3's exit criteria for real: two real, separate Linux processes exchange a real IPC
//! call across a real Unix domain socket, and a call carrying a revoked capability is rejected
//! at the transport boundary (the server's `authenticate` call, which the client cannot bypass
//! or route around, unsandboxed or not -- see `hyperion_capability::WireToken`'s docs on why
//! this holds regardless of whether the client process itself happens to be OS-sandboxed).
//!
//! The server side runs in this test process (it must own the `CapabilityMonitor` that minted
//! and will revoke the token); the client is a genuinely separate, `exec`'d process
//! (`ipc_client_probe`) that only ever receives a bare `WireToken` claim, exactly the realistic
//! shape for a cross-process IPC client with no local monitor of its own.

use std::path::Path;
use std::process::{Command, Stdio};

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId, WireToken};
use hyperion_ipc::Endpoint;

fn client_bin() -> &'static str {
    env!("CARGO_BIN_EXE_ipc_client_probe")
}

/// Spawns one client run against `server`, handling exactly one call as the server would, and
/// returns the client's reported stdout line.
fn run_one_client_call(
    server: &Endpoint,
    server_sock: &Path,
    client_sock: &Path,
    wire_json: &str,
    monitor: &CapabilityMonitor,
) -> String {
    let child = Command::new(client_bin())
        .env("HYPERION_WIRE_TOKEN", wire_json)
        .env("HYPERION_SERVER_SOCK", server_sock)
        .env("HYPERION_CLIENT_SOCK", client_sock)
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn ipc_client_probe");

    // Server-side handling, in this process, *before* waiting on the child: the child's
    // ipc_call blocks on its own socket for a reply, so it won't exit until this runs.
    let incoming = server
        .recv_raw()
        .expect("receive the client's real CALL frame");
    match server.authenticate(&incoming, monitor, RightsMask::WRITE) {
        Ok(call) => {
            assert!(call.is_call, "client sent a CALL, not a NOTIFY");
            server
                .reply(&incoming, b"pong".to_vec())
                .expect("reply to the authenticated call");
        }
        Err(fault) => {
            // authenticate() already sent the rejecting reply to the real client when the
            // frame was itself a CALL -- nothing further to do here.
            let _ = fault;
        }
    }

    let output = child
        .wait_with_output()
        .expect("wait for ipc_client_probe to exit");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn two_real_processes_exchange_a_real_call_and_revocation_is_enforced_at_the_transport() {
    let dir = tempfile::tempdir().expect("create tempdir for real sockets");
    let server_sock = dir.path().join("server.sock");
    let client_sock = dir.path().join("client.sock");

    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::WRITE, TrustBoundaryId(1), None);
    let wire_json = serde_json::to_string(&WireToken::from(&token)).expect("serialize WireToken");

    let server = Endpoint::bind(&server_sock).expect("bind real server socket");

    // Phase 1: the token is live. A real, separate process presenting its wire claim over a
    // real socket gets a real reply.
    let first = run_one_client_call(&server, &server_sock, &client_sock, &wire_json, &monitor);
    assert_eq!(
        first, "CALL_OK:pong",
        "a live capability's real IPC call must succeed end to end"
    );

    // Phase 2: revoke the token in the *server's* monitor -- the only one that matters, since
    // it's the one `authenticate` actually checks against. The client still presents the exact
    // same wire bytes it had before; nothing about its own belief changes.
    monitor.cap_revoke(&token);

    let second = run_one_client_call(&server, &server_sock, &client_sock, &wire_json, &monitor);
    assert!(
        second.starts_with("CALL_ERR:"),
        "a revoked capability's real IPC call must be rejected, got: {second}"
    );
    assert!(
        second.contains("revoked"),
        "rejection should be attributable to revocation specifically, got: {second}"
    );
}
