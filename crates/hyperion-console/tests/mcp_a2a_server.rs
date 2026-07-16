//! Real, end-to-end coverage of `/mcp-server`, `/a2a-server`, `/standby`, `/mcp-call`, and
//! `/a2a-call` (docs/998-roadmap.md's Social pillar) -- spawns the actual compiled binary,
//! connects to its real background HTTP server over a real socket while the process is still
//! running, and drives it with real JSON-RPC requests, exactly the same "spawn the real binary,
//! don't mock it" discipline `tests/scenario_file.rs` already established. This behavior lives in
//! `main.rs` itself (a shared `Arc<Mutex<ConsoleSession>>` and background server threads main.rs
//! alone needs), deliberately kept out of the `ConsoleSession` library API.

use std::io::{Read, Write};
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

/// Blocks until a real socket on `port` actually accepts a connection -- the background server
/// starts in its own real thread after the scenario line that triggers it runs, so a fixed,
/// bounded poll-connect (not a guessed sleep) is what makes this reliable.
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

fn http_post(port: u16, path: &str, body: &str) -> String {
    let mut stream =
        TcpStream::connect(("127.0.0.1", port)).expect("connect to the real running server");
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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

fn http_get(port: u16, path: &str) -> String {
    let mut stream =
        TcpStream::connect(("127.0.0.1", port)).expect("connect to the real running server");
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .expect("send a real HTTP request");
    let mut raw = String::new();
    stream
        .read_to_string(&mut raw)
        .expect("read the real HTTP response");
    raw.split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default()
}

/// Writes a real newline to `child`'s real stdin (unblocking a real `/standby`) and waits for it
/// to actually exit, asserting a clean exit code.
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
        "expected a clean exit after /standby, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn mcp_server_serves_real_tools_over_a_real_http_connection() {
    const PORT: u16 = 18765;
    let dir = tempfile::tempdir().unwrap();
    let child = spawn_scenario(
        dir.path(),
        &format!("my name is Alex\n/mcp-server {PORT}\n/standby\n"),
    );
    wait_for_port(PORT);

    let init = http_post(
        PORT,
        "/",
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    );
    assert!(init.contains("2024-11-05"), "got: {init:?}");

    let tools = http_post(
        PORT,
        "/",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
    );
    assert!(tools.contains("hyperion.ask"), "got: {tools:?}");
    assert!(tools.contains("hyperion.recall"), "got: {tools:?}");
    assert!(tools.contains("hyperion.graph"), "got: {tools:?}");

    // The real proof this is the *same* live session, not a fresh one per call: asking about
    // something stated in the scenario's own first line, before the server ever started.
    let ask = http_post(
        PORT,
        "/",
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"hyperion.ask","arguments":{"prompt":"what is my name"}}}"#,
    );
    assert!(ask.contains("Alex"), "got: {ask:?}");

    let graph = http_post(
        PORT,
        "/",
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"hyperion.graph","arguments":{}}}"#,
    );
    assert!(graph.contains("2 nodes"), "got: {graph:?}");

    let transcript = resume_and_wait(child);
    assert!(
        transcript.contains("Real MCP server listening"),
        "got: {transcript:?}"
    );
}

#[test]
fn a2a_server_serves_a_real_agent_card_and_sendmessage() {
    const PORT: u16 = 18766;
    let dir = tempfile::tempdir().unwrap();
    let child = spawn_scenario(dir.path(), &format!("/a2a-server {PORT}\n/standby\n"));
    wait_for_port(PORT);

    let card = http_get(PORT, "/.well-known/agent-card.json");
    assert!(
        card.contains("\"id\":\"hyperion-console\""),
        "got: {card:?}"
    );
    assert!(card.contains("\"skills\""), "got: {card:?}");

    let response = http_post(
        PORT,
        "/",
        r#"{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":{"messageId":"m1","role":"ROLE_USER","parts":[{"text":"hello there"}]},"configuration":{"returnImmediately":false}}}"#,
    );
    assert!(response.contains("hello there"), "got: {response:?}");
    assert!(
        response.contains("TASK_STATE_COMPLETED"),
        "got: {response:?}"
    );

    resume_and_wait(child);
}

#[test]
fn mcp_call_reaches_a_real_running_mcp_server_from_a_second_process() {
    const PORT: u16 = 18767;
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
        format!("/mcp-call 127.0.0.1 {PORT} hyperion.ask {{\"prompt\":\"say hi\"}}\n"),
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
        transcript.contains("say hi"),
        "expected the real remote server's real echoed reply, got: {transcript:?}"
    );

    resume_and_wait(server);
}

#[test]
fn a2a_call_reaches_a_real_running_a2a_server_from_a_second_process() {
    const PORT: u16 = 18768;
    let dir = tempfile::tempdir().unwrap();
    let server = spawn_scenario(dir.path(), &format!("/a2a-server {PORT}\n/standby\n"));
    wait_for_port(PORT);

    let client_dir = tempfile::tempdir().unwrap();
    let client_scenario = client_dir.path().join("scenario.txt");
    std::fs::write(
        &client_scenario,
        format!("/a2a-call 127.0.0.1 {PORT} hello there\n"),
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
        transcript.contains("hello there"),
        "expected the real remote session's own real reply (echoing what was sent), got: \
         {transcript:?}"
    );

    resume_and_wait(server);
}

#[test]
fn standby_actually_blocks_until_real_input_then_the_process_exits_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = spawn_scenario(dir.path(), "hello there\n/standby\n");

    // A real, bounded wait proving the process is genuinely still alive (blocked in /standby),
    // not just slow -- `try_wait` returns `Ok(None)` for a still-running child.
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        child.try_wait().unwrap().is_none(),
        "the process must still be running, blocked at /standby"
    );

    resume_and_wait(child);
}
