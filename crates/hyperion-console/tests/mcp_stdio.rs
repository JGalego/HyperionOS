//! docs/998-roadmap.md's own named "stdio transport" gap in the MCP spec, closed for real:
//! `--mcp-stdio` really speaks newline-delimited JSON-RPC over this process's own real
//! stdin/stdout -- the transport most real MCP clients actually launch a server with -- through
//! the exact same `handle_request` dispatch `/mcp-server`'s own real HTTP transport uses.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn spawn_stdio_server(data_dir: &std::path::Path) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_hyperion-console"))
        .arg("--mcp-stdio")
        .env("HYPERION_CONSOLE_DATA_DIR", data_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the real compiled hyperion-console binary in --mcp-stdio mode")
}

fn send_and_read(
    stdin: &mut impl Write,
    stdout: &mut impl BufRead,
    request: &serde_json::Value,
) -> serde_json::Value {
    writeln!(stdin, "{request}").expect("write a real line to the real child's stdin");
    stdin.flush().unwrap();
    let mut line = String::new();
    stdout
        .read_line(&mut line)
        .expect("read a real line back from the real child's stdout");
    serde_json::from_str(&line).unwrap_or_else(|e| panic!("not valid JSON ({e}): {line:?}"))
}

#[test]
fn stdio_transport_really_serves_initialize_and_tools_call_over_real_stdin_stdout() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = spawn_stdio_server(dir.path());
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let init = send_and_read(
        &mut stdin,
        &mut stdout,
        &serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
    );
    assert_eq!(init["result"]["protocolVersion"], "2024-11-05");
    assert!(init["result"]["publicKey"].as_str().is_some());

    let call = send_and_read(
        &mut stdin,
        &mut stdout,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": "hyperion.ask", "arguments": {"prompt": "say hi over stdio"}},
        }),
    );
    let text = call["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("say hi over stdio"), "got: {text:?}");
    assert!(
        call["result"]["signature"].as_str().is_some(),
        "got: {call:?}"
    );

    drop(stdin); // real EOF -- the real, honest way this transport's own loop ends.
    let output = child
        .wait_with_output()
        .expect("wait for a real clean exit");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn stdio_transport_serves_resources_too() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = spawn_stdio_server(dir.path());
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let list = send_and_read(
        &mut stdin,
        &mut stdout,
        &serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "resources/list", "params": {}}),
    );
    let resources = list["result"]["resources"].as_array().unwrap();
    assert!(
        resources.iter().any(|r| r["uri"] == "hyperion://graph"),
        "got: {resources:?}"
    );

    drop(stdin);
    let output = child
        .wait_with_output()
        .expect("wait for a real clean exit");
    assert!(output.status.success());
}
