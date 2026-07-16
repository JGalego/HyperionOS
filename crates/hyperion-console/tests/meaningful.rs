//! docs/998-roadmap.md's Backlog "Protect the Human" item: "no 'was this meaningful' signal, only
//! 'was this fast.'" `/meaningful yes|no` really persists a reflection, real end to end through
//! this console's own Knowledge Graph -- and carries no timing/speed data at all, unlike
//! `hyperion-observability`'s own latency tracking.

use hyperion_console::ConsoleSession;

fn open_session() -> (tempfile::TempDir, ConsoleSession) {
    let dir = tempfile::tempdir().expect("create a real tempdir for this test's Knowledge Graph");
    let session = ConsoleSession::open(dir.path()).expect("open a real ConsoleSession");
    (dir, session)
}

#[test]
fn meaningful_with_nothing_to_reflect_on_says_so_honestly() {
    let (_dir, mut session) = open_session();
    let reply = session.handle_utterance("/meaningful yes");
    assert!(
        reply.iter().any(|l| l.to_lowercase().contains("nothing")),
        "expected an honest \"nothing to reflect on\" reply, got: {reply:?}"
    );
}

#[test]
fn bare_meaningful_asks_about_the_last_real_goal() {
    let (_dir, mut session) = open_session();
    session.handle_utterance("I need to launch my startup");

    let reply = session.handle_utterance("/meaningful");
    let joined = reply.join("\n");
    assert!(
        joined.contains("launch my startup"),
        "expected the prompt to reference the last real goal, got: {reply:?}"
    );
}

#[test]
fn meaningful_yes_really_persists_a_reflection() {
    let (_dir, mut session) = open_session();
    session.handle_utterance("I need to launch my startup");

    let reply = session.handle_utterance("/meaningful yes");
    assert!(
        reply.iter().any(|l| l.to_lowercase().contains("noted")),
        "expected a real confirmation, got: {reply:?}"
    );

    let recalled = session.handle_utterance("/recall reflection");
    let joined = recalled.join("\n");
    assert!(
        joined.to_lowercase().contains("memory"),
        "expected the reflection to be really findable through this console's own memory, got: \
         {recalled:?}"
    );
}

#[test]
fn an_unrecognized_meaningful_argument_is_rejected_honestly() {
    let (_dir, mut session) = open_session();
    session.handle_utterance("I need to launch my startup");

    let reply = session.handle_utterance("/meaningful sort-of");
    assert!(
        reply
            .iter()
            .any(|l| l.contains("isn't something I know how to record")),
        "got: {reply:?}"
    );
}
