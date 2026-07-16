//! docs/998-roadmap.md's Backlog "Protect the Human" item, end to end through the real console
//! pipeline: `/think on` really pauses the next decomposable utterance instead of acting on it
//! immediately, and `/think-proceed` really commits to it once the human says so.

use hyperion_console::ConsoleSession;

fn open_session() -> (tempfile::TempDir, ConsoleSession) {
    let dir = tempfile::tempdir().expect("create a real tempdir for this test's Knowledge Graph");
    let session = ConsoleSession::open(dir.path()).expect("open a real ConsoleSession");
    (dir, session)
}

#[test]
fn think_mode_is_off_by_default() {
    let (_dir, mut session) = open_session();
    let lines = session.handle_utterance("/think");
    assert!(
        lines.iter().any(|l| l.contains("off")),
        "expected think mode to report off by default, got: {lines:?}"
    );
}

#[test]
fn think_on_pauses_a_decomposable_utterance_instead_of_acting_on_it() {
    let (_dir, mut session) = open_session();

    let on_reply = session.handle_utterance("/think on");
    assert!(
        on_reply.iter().any(|l| l.to_lowercase().contains("on")),
        "expected confirmation that think mode is on, got: {on_reply:?}"
    );

    let paused = session.handle_utterance("I need to launch my startup");
    let joined = paused.join("\n");
    assert!(
        joined.to_lowercase().contains("think-proceed"),
        "a paused utterance must point the user at \"/think-proceed\", got: {paused:?}"
    );
    // The real decomposition must NOT have happened yet -- none of the real HTN template's own
    // leaves should appear in this reply.
    assert!(
        !joined.contains("market_research"),
        "think mode must withhold decomposition until told to proceed, got: {paused:?}"
    );
}

#[test]
fn think_proceed_completes_the_paused_decomposition() {
    let (_dir, mut session) = open_session();
    session.handle_utterance("/think on");
    session.handle_utterance("I need to launch my startup");

    let proceed_reply = session.handle_utterance("/think-proceed");
    assert!(
        proceed_reply
            .iter()
            .any(|l| l.to_lowercase().contains("proceeding")),
        "expected a real confirmation that decomposition is proceeding, got: {proceed_reply:?}"
    );
}

#[test]
fn think_proceed_with_nothing_pending_says_so_honestly() {
    let (_dir, mut session) = open_session();
    let reply = session.handle_utterance("/think-proceed");
    assert!(
        reply.iter().any(|l| l.to_lowercase().contains("nothing")),
        "expected an honest \"nothing pending\" reply, got: {reply:?}"
    );
}

#[test]
fn think_off_lets_the_next_utterance_decompose_immediately_again() {
    let (_dir, mut session) = open_session();
    session.handle_utterance("/think on");
    session.handle_utterance("/think off");

    let lines = session.handle_utterance("I need to launch my startup");
    let joined = lines.join("\n");
    assert!(
        joined.contains("market_research"),
        "think mode off must decompose immediately as before, got: {lines:?}"
    );
}
