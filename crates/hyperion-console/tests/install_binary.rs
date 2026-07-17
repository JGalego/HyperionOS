//! `AgentRuntime::invoke`'s real, Landlock/seccomp-sandboxed `NativeBinary` capability dispatch
//! previously had no reachable production caller -- every real binary in this workspace
//! constructed `AgentRuntime` with `plugins: None`. `/install-binary <capability id> <program>`
//! is the real, explicit invocation that makes it reachable, real end to end through
//! `ConsoleSession`'s own real `PluginRegistry`.

use hyperion_console::ConsoleSession;

fn open_session() -> (tempfile::TempDir, ConsoleSession) {
    let dir = tempfile::tempdir().expect("create a real tempdir for this test's Knowledge Graph");
    let session = ConsoleSession::open(dir.path()).expect("open a real ConsoleSession");
    (dir, session)
}

#[test]
fn install_binary_with_missing_args_asks_for_both() {
    let (_dir, mut session) = open_session();
    let reply = session.handle_utterance("/install-binary");
    assert!(
        reply
            .iter()
            .any(|l| l.contains("needs a capability id and a program path")),
        "got: {reply:?}"
    );

    let reply = session.handle_utterance("/install-binary local.only_one_arg");
    assert!(
        reply
            .iter()
            .any(|l| l.contains("needs a capability id and a program path")),
        "a bare capability id with no program path must still be rejected, got: {reply:?}"
    );
}

#[test]
fn install_binary_with_a_real_executable_succeeds() {
    let (_dir, mut session) = open_session();
    let reply = session
        .handle_utterance("/install-binary local.word_count /usr/bin/wc")
        .join("\n");
    assert!(
        reply.contains("Installed") && reply.contains("local.word_count"),
        "expected a real success message, got: {reply}"
    );
}

#[test]
fn install_binary_rejects_a_program_that_does_not_exist() {
    let (_dir, mut session) = open_session();
    let reply = session
        .handle_utterance("/install-binary local.ghost /no/such/program/anywhere")
        .join("\n");
    assert!(
        reply.contains("couldn't install"),
        "a nonexistent program must fail real install validation, not silently succeed: {reply}"
    );
}

#[test]
fn install_binary_rejects_a_non_executable_file() {
    let (dir, mut session) = open_session();
    let not_executable = dir.path().join("not_executable.txt");
    std::fs::write(&not_executable, b"not a program").expect("write a real, non-executable file");
    let reply = session
        .handle_utterance(&format!(
            "/install-binary local.not_a_program {}",
            not_executable.display()
        ))
        .join("\n");
    assert!(
        reply.contains("couldn't install"),
        "a real, non-executable file must fail real install validation: {reply}"
    );
}
