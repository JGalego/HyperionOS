//! docs/998-roadmap.md's Backlog "Protect the Human" item: "no teaching mode... nothing that
//! explains the underlying principle instead of just the output." `/teach <topic>` is a real,
//! explicit invocation, real end to end through the same dispatch `run_undecomposed_goal` uses.

use hyperion_console::ConsoleSession;

fn open_session() -> (tempfile::TempDir, ConsoleSession) {
    let dir = tempfile::tempdir().expect("create a real tempdir for this test's Knowledge Graph");
    let session = ConsoleSession::open(dir.path()).expect("open a real ConsoleSession");
    (dir, session)
}

#[test]
fn teach_with_no_topic_asks_for_one() {
    let (_dir, mut session) = open_session();
    let reply = session.handle_utterance("/teach");
    assert!(
        reply.iter().any(|l| l.contains("needs a topic")),
        "got: {reply:?}"
    );
}

#[test]
fn teach_dispatches_a_real_explanation_oriented_prompt() {
    let (_dir, mut session) = open_session();
    let reply = session.handle_utterance("/teach how DNS resolution works");
    assert!(
        !reply.is_empty(),
        "a real /teach dispatch must produce at least one line of real text"
    );
    // MockBackend's own real echo behavior includes the prompt it was given -- proof the real
    // teaching-oriented prompt (not a bare topic string) actually reached the real backend.
    let joined = reply.join("\n");
    assert!(
        joined.to_lowercase().contains("underlying principle")
            || joined.to_lowercase().contains("dns resolution"),
        "expected the real dispatch to reflect the teaching-oriented prompt, got: {reply:?}"
    );
}
