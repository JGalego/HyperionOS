//! Real, many-instance Hyperion-to-Hyperion capability delegation (docs/998-roadmap.md's Social
//! pillar) -- spawns two real, separately-started `hyperion-console` processes, each with a
//! different real `HYPERION_CONSOLE_CAPABILITIES`, and proves one really discovers the other over
//! real mDNS and really delegates a task it can't do itself, exactly the same "spawn the real
//! binary, don't mock it" discipline `tests/mcp_a2a_server.rs`/`tests/mdns_discovery.rs` already
//! established. Two disjoint test sets, one per build configuration, matching
//! `tests/mdns_discovery.rs`'s own shape.

use std::io::Write;
use std::process::{Command, Stdio};

fn spawn_scenario(
    dir: &std::path::Path,
    scenario: &str,
    capabilities: &str,
) -> std::process::Child {
    let scenario_path = dir.join("scenario.txt");
    std::fs::write(&scenario_path, scenario).expect("write a real scenario file");
    Command::new(env!("CARGO_BIN_EXE_hyperion-console"))
        .arg(&scenario_path)
        .env("HYPERION_CONSOLE_DATA_DIR", dir.join("data"))
        .env("HYPERION_CONSOLE_CAPABILITIES", capabilities)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the real compiled hyperion-console binary")
}

/// Writes a real newline to `child`'s real stdin (unblocking a real `/standby`) and waits for it
/// to actually exit, asserting a clean exit code -- same helper this crate's other server tests
/// already use.
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

#[cfg(feature = "mdns")]
mod real {
    use super::*;
    use std::io::Read;
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

    /// Blocks until a real socket on `port` actually accepts a connection -- same discipline
    /// `tests/mcp_a2a_server.rs` already uses.
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

    #[test]
    fn a_node_lacking_a_capability_really_discovers_and_delegates_to_a_real_peer_that_has_it() {
        const PROVIDER_PORT: u16 = 18830;
        const REQUESTER_PORT: u16 = 18831;

        let provider_dir = tempfile::tempdir().unwrap();
        let provider = spawn_scenario(
            provider_dir.path(),
            &format!("/a2a-server {PROVIDER_PORT} node-a\n/standby\n"),
            "mesh-test-translate-ja",
        );
        wait_for_port(PROVIDER_PORT);

        // The provider's own Agent Card really reflects its configured capability, not the old
        // hardcoded default.
        let card = http_get(PROVIDER_PORT, "/.well-known/agent-card.json");
        assert!(
            card.contains("\"id\":\"mesh-test-translate-ja\""),
            "got: {card:?}"
        );

        let requester_dir = tempfile::tempdir().unwrap();
        let requester = spawn_scenario(
            requester_dir.path(),
            &format!(
                "/a2a-server {REQUESTER_PORT} node-b\n/mesh-request {REQUESTER_PORT} \
                 mesh-test-translate-ja hello there\n/standby\n"
            ),
            "hyperion.ask",
        );
        wait_for_port(REQUESTER_PORT);

        let requester_transcript = resume_and_wait(requester);
        assert!(
            requester_transcript.contains("I don't have \"mesh-test-translate-ja\" myself"),
            "expected an honest explanation of why it delegated, got: {requester_transcript:?}"
        );
        assert!(
            requester_transcript.contains("asked node-a"),
            "expected the real chosen peer's own instance name, got: {requester_transcript:?}"
        );
        assert!(
            requester_transcript.contains("hello there"),
            "expected the real remote reply (echoing what was sent), got: \
             {requester_transcript:?}"
        );

        // The provider's own `/mesh/status` really recorded having been asked, from its own side.
        let status = http_get(PROVIDER_PORT, "/mesh/status");
        assert!(status.contains("\"DelegationReceived\""), "got: {status:?}");
        assert!(
            status.contains("hello there"),
            "expected the real request text in the provider's own event log, got: {status:?}"
        );

        resume_and_wait(provider);
    }
}

#[cfg(not(feature = "mdns"))]
mod fallback {
    use super::*;

    #[test]
    fn without_the_mdns_feature_mesh_request_degrades_honestly() {
        let dir = tempfile::tempdir().unwrap();
        let child = spawn_scenario(
            dir.path(),
            "/mesh-request 18832 mesh-test-translate-ja hello there\n/standby\n",
            "hyperion.ask",
        );
        let transcript = resume_and_wait(child);
        assert!(
            transcript
                .contains("I couldn't find anyone on the LAN with \"mesh-test-translate-ja\""),
            "got: {transcript:?}"
        );
        assert!(
            transcript.contains("wasn't built with the \"mdns\" feature"),
            "a binary built without the mdns feature must say so honestly, got: {transcript:?}"
        );
    }
}
