//! Real mDNS/DNS-SD advertise+discover (docs/998-roadmap.md's Social pillar) -- spawns the
//! actual compiled binary, exactly the same "spawn the real binary, don't mock it" discipline
//! `tests/mcp_a2a_server.rs` already established. Two disjoint test sets, one per build
//! configuration: with the `mdns` feature, `/mcp-server`'s own real advertisement is really
//! discoverable via `/mcp-discover`; without it, both commands degrade to an honest, real error
//! message instead of a silently faked result.

use std::process::{Command, Stdio};

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

/// Writes a real newline to `child`'s real stdin (unblocking a real `/standby`) and waits for it
/// to actually exit, asserting a clean exit code -- same helper `tests/mcp_a2a_server.rs` uses.
fn resume_and_wait(mut child: std::process::Child) -> String {
    use std::io::Write;
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

    #[test]
    fn a_running_mcp_server_is_really_discoverable_via_mdns() {
        const PORT: u16 = 18780;
        let dir = tempfile::tempdir().unwrap();
        let child = spawn_scenario(
            dir.path(),
            &format!("/mcp-server {PORT}\n/mcp-discover 2\n/standby\n"),
        );
        let transcript = resume_and_wait(child);
        assert!(
            transcript.contains("Also advertising as \"hyperion-mcp\""),
            "got: {transcript:?}"
        );
        assert!(
            transcript.contains(&format!(":{PORT}")),
            "the real discovered peer must list the real port it's really listening on, got: \
             {transcript:?}"
        );
        // A real multi-homed host can legitimately resolve one advertised instance to several
        // real addresses (one per real interface/address family) -- "at least one" is the real
        // invariant, not "exactly one".
        assert!(
            transcript.contains("Found ") && transcript.contains("real peer(s) for"),
            "got: {transcript:?}"
        );
        assert!(
            transcript.contains("hyperion-mcp._hyperion-mcp._tcp.local."),
            "got: {transcript:?}"
        );
    }

    #[test]
    fn discovering_with_no_real_peer_advertising_finds_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let child = spawn_scenario(dir.path(), "/a2a-discover 1\n/standby\n");
        let transcript = resume_and_wait(child);
        assert!(
            transcript.contains("No real peers answered"),
            "got: {transcript:?}"
        );
    }
}

#[cfg(not(feature = "mdns"))]
mod fallback {
    use super::*;

    #[test]
    fn without_the_mdns_feature_advertise_and_discover_degrade_honestly() {
        const PORT: u16 = 18781;
        let dir = tempfile::tempdir().unwrap();
        let child = spawn_scenario(
            dir.path(),
            &format!("/mcp-server {PORT}\n/mcp-discover 1\n/standby\n"),
        );
        let transcript = resume_and_wait(child);
        assert!(
            transcript.contains("Not advertised on the LAN"),
            "a binary built without the mdns feature must say so honestly, not silently \
             pretend to have advertised, got: {transcript:?}"
        );
        assert!(
            transcript.contains("I couldn't scan the LAN"),
            "got: {transcript:?}"
        );
    }
}
