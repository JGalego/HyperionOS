//! docs/998-roadmap.md's Social pillar: real cross-instance identity for `/mcp-call`, the same
//! trust-on-first-use shape `tests/a2a_identity.rs` already proves for `/a2a-call` -- a real
//! `initialize` response now carries a real Ed25519 public key, a real `tools/call` reply is
//! really signed over with it, and the client really verifies both against a real, persisted
//! trust store, refusing a reply from a peer that suddenly presents a different key.

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

fn mcp_call_line(port: u16) -> String {
    format!("/mcp-call 127.0.0.1 {port} hyperion.ask {{\"prompt\":\"what is my name\"}}")
}

#[test]
fn a_first_call_trusts_the_peers_identity_and_a_second_call_confirms_it_silently() {
    const PORT: u16 = 18800;
    let server_dir = tempfile::tempdir().unwrap();
    let client_dir = tempfile::tempdir().unwrap();

    let server = spawn_scenario(
        &server_dir.path().join("data"),
        &format!("my name is Alex\n/mcp-server {PORT}\n/standby\n"),
    );
    wait_for_port(PORT);

    let first = client_call(&client_dir.path().join("data"), &mcp_call_line(PORT));
    assert!(first.contains("Alex"), "got: {first:?}");
    assert!(
        first.contains("Trusting 127.0.0.1:18800's identity for the first time"),
        "got: {first:?}"
    );

    let second = client_call(&client_dir.path().join("data"), &mcp_call_line(PORT));
    assert!(second.contains("Alex"), "got: {second:?}");
    assert!(
        !second.contains("Trusting"),
        "a repeat contact with the same identity must be silently confirmed, got: {second:?}"
    );

    resume_and_wait(server);
}

#[test]
fn a_different_server_answering_the_same_address_is_refused_not_silently_trusted() {
    const PORT: u16 = 18801;
    let client_dir = tempfile::tempdir().unwrap();

    let server_a_dir = tempfile::tempdir().unwrap();
    let server_a = spawn_scenario(
        &server_a_dir.path().join("data"),
        &format!("my name is Alex\n/mcp-server {PORT}\n/standby\n"),
    );
    wait_for_port(PORT);
    let first = client_call(&client_dir.path().join("data"), &mcp_call_line(PORT));
    assert!(first.contains("Alex"), "got: {first:?}");
    resume_and_wait(server_a);

    let server_b_dir = tempfile::tempdir().unwrap();
    let server_b = spawn_scenario(
        &server_b_dir.path().join("data"),
        &format!("my name is Mallory\n/mcp-server {PORT}\n/standby\n"),
    );
    wait_for_port(PORT);
    let second = client_call(&client_dir.path().join("data"), &mcp_call_line(PORT));
    assert!(
        second.contains("WARNING") && second.contains("DIFFERENT identity"),
        "got: {second:?}"
    );
    assert!(
        !second.contains("Mallory"),
        "the impersonating server's real reply must never be shown, got: {second:?}"
    );

    let forget_output = client_call(
        &client_dir.path().join("data"),
        &format!("/trust forget 127.0.0.1:{PORT}"),
    );
    assert!(forget_output.contains("Forgot"), "got: {forget_output:?}");

    let third = client_call(&client_dir.path().join("data"), &mcp_call_line(PORT));
    assert!(
        third.contains("Mallory"),
        "after an explicit /trust forget, the real reply must go through again, got: {third:?}"
    );

    resume_and_wait(server_b);
}
