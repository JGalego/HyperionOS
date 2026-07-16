//! docs/998-roadmap.md's Social pillar: real cross-instance identity for `/a2a-call`. A real
//! Agent Card now carries a real Ed25519 public key; a real `SendMessage` reply is really signed
//! over with it; the client really verifies that signature and really checks the key against a
//! persisted trust-on-first-use store -- proven end to end by swapping which real server answers
//! the same host:port between two real, differently-keyed `hyperion-console` processes and
//! confirming the second one's reply is refused, not silently shown.

use std::io::Write;
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn spawn_scenario(server_data_dir: &std::path::Path, scenario: &str) -> std::process::Child {
    let dir = server_data_dir.parent().unwrap();
    let scenario_path = dir.join(format!(
        "scenario-{}.txt",
        server_data_dir.file_name().unwrap().to_string_lossy()
    ));
    std::fs::write(&scenario_path, scenario).expect("write a real scenario file");
    Command::new(env!("CARGO_BIN_EXE_hyperion-console"))
        .arg(&scenario_path)
        .env("HYPERION_CONSOLE_DATA_DIR", server_data_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the real compiled hyperion-console binary")
}

fn wait_for_port(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        if Instant::now() >= deadline {
            panic!("no real server ever accepted a connection on port {port}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn resume_and_wait(mut child: std::process::Child) -> String {
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(b"\n")
        .expect("write a real line to unblock /standby");
    let output = child
        .wait_with_output()
        .expect("wait for a real clean exit");
    assert!(
        output.status.success(),
        "expected a clean exit, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Runs a single real client scenario line against an already-running server, sharing
/// `client_data_dir`'s own real, persisted peer trust store across every call to this helper.
fn client_call(client_data_dir: &std::path::Path, scenario_line: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let scenario_path = dir.path().join("scenario.txt");
    std::fs::write(&scenario_path, format!("{scenario_line}\n")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_hyperion-console"))
        .arg(&scenario_path)
        .env("HYPERION_CONSOLE_DATA_DIR", client_data_dir)
        .output()
        .expect("spawn a real hyperion-console client process");
    assert!(output.status.success(), "{:?}", output.stderr);
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn a_first_call_trusts_the_peers_identity_and_a_second_call_confirms_it_silently() {
    const PORT: u16 = 18790;
    let server_dir = tempfile::tempdir().unwrap();
    let client_dir = tempfile::tempdir().unwrap();

    let server = spawn_scenario(
        &server_dir.path().join("data"),
        &format!("/a2a-server {PORT}\n/standby\n"),
    );
    wait_for_port(PORT);

    let first = client_call(
        &client_dir.path().join("data"),
        &format!("/a2a-call 127.0.0.1 {PORT} hello there"),
    );
    assert!(first.contains("hello there"), "got: {first:?}");
    assert!(
        first.contains("Trusting 127.0.0.1:18790's identity for the first time"),
        "got: {first:?}"
    );

    let second = client_call(
        &client_dir.path().join("data"),
        &format!("/a2a-call 127.0.0.1 {PORT} hello there"),
    );
    assert!(second.contains("hello there"), "got: {second:?}");
    assert!(
        !second.contains("Trusting"),
        "a repeat contact with the same identity must be silently confirmed, got: {second:?}"
    );

    resume_and_wait(server);
}

#[test]
fn a_different_server_answering_the_same_address_is_refused_not_silently_trusted() {
    const PORT: u16 = 18791;
    let client_dir = tempfile::tempdir().unwrap();
    // Each `SendMessage` is a real, stateless, one-off dispatch (see
    // `ConsoleSession::handle_utterance_stateless`'s own doc comment) -- there's no shared
    // conversation memory left to distinguish "which real server answered" through, so the
    // distinguishing marker rides in the outgoing message itself instead, echoed straight back by
    // whichever real `MockBackend` actually received and answered it.
    const MARKER: &str = "marker-for-the-real-server-that-should-answer";

    let server_a_dir = tempfile::tempdir().unwrap();
    let server_a = spawn_scenario(
        &server_a_dir.path().join("data"),
        &format!("/a2a-server {PORT}\n/standby\n"),
    );
    wait_for_port(PORT);
    let first = client_call(
        &client_dir.path().join("data"),
        &format!("/a2a-call 127.0.0.1 {PORT} {MARKER}"),
    );
    // "echo: {MARKER}" (the real `MockBackend` reply's own shape), not bare `MARKER` -- the
    // outgoing command line itself is also echoed to this same transcript, so a bare substring
    // check can't tell "the real reply repeated it" from "the command we typed contained it".
    let echoed_reply = format!("echo: {MARKER}");
    assert!(first.contains(&echoed_reply), "got: {first:?}");
    resume_and_wait(server_a);

    // A real, differently-keyed second server (its own fresh data dir -> its own fresh device
    // identity) answering the exact same host:port -- the real impersonation-after-the-fact
    // scenario this identity check exists to catch.
    let server_b_dir = tempfile::tempdir().unwrap();
    let server_b = spawn_scenario(
        &server_b_dir.path().join("data"),
        &format!("/a2a-server {PORT}\n/standby\n"),
    );
    wait_for_port(PORT);
    let second = client_call(
        &client_dir.path().join("data"),
        &format!("/a2a-call 127.0.0.1 {PORT} {MARKER}"),
    );
    assert!(
        second.contains("WARNING") && second.contains("DIFFERENT identity"),
        "got: {second:?}"
    );
    assert!(
        !second.contains(&echoed_reply),
        "the impersonating server's real reply must never be shown, got: {second:?}"
    );

    // The real, explicit override: after investigating, the user re-trusts the new key.
    let forget_output = client_call(
        &client_dir.path().join("data"),
        &format!("/trust forget 127.0.0.1:{PORT}"),
    );
    assert!(forget_output.contains("Forgot"), "got: {forget_output:?}");

    let third = client_call(
        &client_dir.path().join("data"),
        &format!("/a2a-call 127.0.0.1 {PORT} {MARKER}"),
    );
    assert!(
        third.contains(&echoed_reply),
        "after an explicit /trust forget, the real reply must go through again, got: {third:?}"
    );
    assert!(
        third.contains("Trusting"),
        "re-trusting after forgetting must be a fresh first-trust, got: {third:?}"
    );

    resume_and_wait(server_b);
}

#[test]
fn trust_list_shows_a_really_trusted_peer() {
    const PORT: u16 = 18792;
    let client_dir = tempfile::tempdir().unwrap();
    let server_dir = tempfile::tempdir().unwrap();

    let server = spawn_scenario(
        &server_dir.path().join("data"),
        &format!("/a2a-server {PORT}\n/standby\n"),
    );
    wait_for_port(PORT);
    client_call(
        &client_dir.path().join("data"),
        &format!("/a2a-call 127.0.0.1 {PORT} hello"),
    );

    let listing = client_call(&client_dir.path().join("data"), "/trust list");
    assert!(
        listing.contains(&format!("127.0.0.1:{PORT}")),
        "got: {listing:?}"
    );

    resume_and_wait(server);
}
