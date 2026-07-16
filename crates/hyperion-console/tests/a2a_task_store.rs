//! docs/998-roadmap.md's own named "GetTask/ListTasks" gap in the A2A spec, closed for real: a
//! real, in-process, insertion-ordered task store keeps every completed `SendMessage` `Task`,
//! and `GetTask`/`ListTasks` really query it.

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

fn send_message(port: u16, id: u64, text: &str) -> serde_json::Value {
    let body = http_post(
        port,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "SendMessage",
            "params": {
                "message": {"messageId": format!("m{id}"), "role": "ROLE_USER", "parts": [{"text": text}]},
                "configuration": {"returnImmediately": false},
            },
        })
        .to_string(),
    );
    serde_json::from_str(&body).expect("a real JSON-RPC response")
}

#[test]
fn a_completed_task_can_really_be_re_fetched_by_get_task() {
    const PORT: u16 = 18820;
    let dir = tempfile::tempdir().unwrap();
    let child = spawn_scenario(dir.path(), &format!("/a2a-server {PORT}\n/standby\n"));
    wait_for_port(PORT);

    let sent = send_message(PORT, 1, "hello there");
    let task_id = sent["result"]["id"]
        .as_str()
        .expect("a real task id")
        .to_string();

    let get_body = http_post(
        PORT,
        &serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "GetTask", "params": {"id": task_id},
        })
        .to_string(),
    );
    let fetched: serde_json::Value = serde_json::from_str(&get_body).unwrap();
    assert_eq!(fetched["result"]["id"], task_id);
    assert!(
        fetched["result"]["status"]["message"]["parts"][0]["text"]
            .as_str()
            .unwrap()
            .contains("hello there"),
        "got: {fetched:?}"
    );

    resume_and_wait(child);
}

#[test]
fn getting_an_unknown_task_is_a_real_honest_error() {
    const PORT: u16 = 18821;
    let dir = tempfile::tempdir().unwrap();
    let child = spawn_scenario(dir.path(), &format!("/a2a-server {PORT}\n/standby\n"));
    wait_for_port(PORT);

    let body = http_post(
        PORT,
        &serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "GetTask", "params": {"id": "no-such-task"},
        })
        .to_string(),
    );
    let response: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(response.get("error").is_some(), "got: {response:?}");

    resume_and_wait(child);
}

#[test]
fn list_tasks_really_lists_every_completed_task_in_order() {
    const PORT: u16 = 18822;
    let dir = tempfile::tempdir().unwrap();
    let child = spawn_scenario(dir.path(), &format!("/a2a-server {PORT}\n/standby\n"));
    wait_for_port(PORT);

    send_message(PORT, 1, "first message");
    send_message(PORT, 2, "second message");

    let list_body = http_post(
        PORT,
        &serde_json::json!({"jsonrpc": "2.0", "id": 3, "method": "ListTasks", "params": {}})
            .to_string(),
    );
    let list: serde_json::Value = serde_json::from_str(&list_body).unwrap();
    let tasks = list["result"]["tasks"].as_array().expect("a real array");
    assert_eq!(tasks.len(), 2, "got: {tasks:?}");
    assert!(
        tasks[0]["status"]["message"]["parts"][0]["text"]
            .as_str()
            .unwrap()
            .contains("first message"),
        "got: {:?}",
        tasks[0]
    );

    resume_and_wait(child);
}
