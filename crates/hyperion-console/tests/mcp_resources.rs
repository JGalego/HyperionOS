//! docs/998-roadmap.md's own named "resources" gap in the MCP spec, closed for real:
//! `resources/list` really lists this session's own real, read-only views; `resources/read`
//! really reads one, through the exact same `ConsoleSession::handle_utterance` path
//! `tools/call` already uses, checked against the same real peer identity `/mcp-call` uses.

use std::io::Write;
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn spawn_scenario(dir: &std::path::Path, scenario: &str) -> std::process::Child {
    let scenario_path = dir.join("scenario.txt");
    std::fs::write(&scenario_path, scenario).expect("write a real scenario file");
    Command::new(env!("CARGO_BIN_EXE_hyperion-console"))
        .arg(&scenario_path)
        .env("HYPERION_CONSOLE_DATA_DIR", dir.join("data"))
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

fn http_post(port: u16, body: &str) -> String {
    use std::io::Read;
    let mut stream =
        TcpStream::connect(("127.0.0.1", port)).expect("connect to the real running server");
    let request = format!(
        "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .expect("send a real HTTP request");
    let mut raw = String::new();
    stream
        .read_to_string(&mut raw)
        .expect("read the real HTTP response");
    raw.split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default()
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
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn resources_list_names_the_real_graph_and_recall_resources() {
    const PORT: u16 = 18810;
    let dir = tempfile::tempdir().unwrap();
    let child = spawn_scenario(dir.path(), &format!("/mcp-server {PORT}\n/standby\n"));
    wait_for_port(PORT);

    let list = http_post(
        PORT,
        r#"{"jsonrpc":"2.0","id":1,"method":"resources/list","params":{}}"#,
    );
    assert!(list.contains("hyperion://graph"), "got: {list:?}");
    assert!(list.contains("hyperion://recall"), "got: {list:?}");

    resume_and_wait(child);
}

#[test]
fn resources_read_returns_the_real_live_graph_content() {
    const PORT: u16 = 18811;
    let dir = tempfile::tempdir().unwrap();
    let child = spawn_scenario(
        dir.path(),
        &format!("my name is Alex\n/mcp-server {PORT}\n/standby\n"),
    );
    wait_for_port(PORT);

    let read = http_post(
        PORT,
        r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"hyperion://graph"}}"#,
    );
    assert!(read.contains("node"), "got: {read:?}");

    resume_and_wait(child);
}

#[test]
fn reading_an_unknown_resource_is_a_real_honest_error() {
    const PORT: u16 = 18812;
    let dir = tempfile::tempdir().unwrap();
    let child = spawn_scenario(dir.path(), &format!("/mcp-server {PORT}\n/standby\n"));
    wait_for_port(PORT);

    let read = http_post(
        PORT,
        r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"hyperion://no-such-thing"}}"#,
    );
    assert!(read.contains("\"error\""), "got: {read:?}");
    assert!(read.contains("unknown resource"), "got: {read:?}");

    resume_and_wait(child);
}

#[test]
fn mcp_resource_reaches_a_real_running_server_and_checks_its_real_identity() {
    const PORT: u16 = 18813;
    let dir = tempfile::tempdir().unwrap();
    let server = spawn_scenario(
        dir.path(),
        &format!("my name is Alex\n/mcp-server {PORT}\n/standby\n"),
    );
    wait_for_port(PORT);

    let client_dir = tempfile::tempdir().unwrap();
    let client_scenario = client_dir.path().join("scenario.txt");
    std::fs::write(
        &client_scenario,
        format!("/mcp-resource 127.0.0.1 {PORT} hyperion://recall\n"),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_hyperion-console"))
        .arg(&client_scenario)
        .env("HYPERION_CONSOLE_DATA_DIR", client_dir.path().join("data"))
        .output()
        .expect("spawn a second real hyperion-console process as the client");
    assert!(output.status.success());
    let transcript = String::from_utf8_lossy(&output.stdout);
    assert!(
        transcript.contains("Trusting 127.0.0.1:18813's identity for the first time"),
        "got: {transcript:?}"
    );

    resume_and_wait(server);
}
