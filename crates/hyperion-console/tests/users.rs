//! Per-person separation in a real console session (docs/998-roadmap.md §0, Decision 2).
//!
//! What is proven here is separation, not protection: nothing authenticates anyone, and these
//! tests assert that the console says so rather than implying otherwise.

use hyperion_console::ConsoleSession;

fn open_as(dir: &tempfile::TempDir, user: &str) -> ConsoleSession {
    ConsoleSession::open_as(dir.path(), user).expect("open a real ConsoleSession")
}

#[test]
fn whoami_names_the_person_and_says_it_is_not_a_login() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = open_as(&dir, "alice");
    let reply = session.handle_utterance("/whoami").join("\n");

    assert!(reply.contains("alice"), "got: {reply}");
    // Someone reading "you are alice" assumes it was checked. It wasn't, and the console has to
    // say so at the moment they ask.
    assert!(
        reply.contains("nothing checks that you are who you say"),
        "got: {reply}"
    );
}

#[test]
fn two_people_on_one_device_do_not_share_working_memory() {
    // The defect this exists for: `session_id` was the literal string "console", and working
    // memory, context bundles and expertise estimates are all keyed by it -- so one person's
    // working memory was recalled into another person's turn.
    let dir = tempfile::tempdir().unwrap();
    {
        let mut alice = open_as(&dir, "alice");
        alice.handle_utterance("my favourite colour is heliotrope");
    }
    let mut bob = open_as(&dir, "bob");
    let reply = bob
        .handle_utterance("what is my favourite colour")
        .join("\n");

    assert!(
        !reply.to_lowercase().contains("heliotrope"),
        "Bob's turn must not see Alice's working memory, got: {reply}"
    );
}

#[test]
fn switching_user_really_changes_who_you_are() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = open_as(&dir, "alice");

    let switched = session.handle_utterance("/user bob").join("\n");
    assert!(switched.contains("bob"), "got: {switched}");
    assert!(
        switched.contains("alice"),
        "should name who you were: {switched}"
    );

    let who = session.handle_utterance("/whoami").join("\n");
    assert!(who.contains("bob"), "got: {who}");
}

#[test]
fn switching_back_and_forth_keeps_each_persons_history_apart() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = open_as(&dir, "alice");
    session.handle_utterance("my favourite colour is heliotrope");

    session.handle_utterance("/user bob");
    let as_bob = session
        .handle_utterance("what is my favourite colour")
        .join("\n");
    assert!(
        !as_bob.to_lowercase().contains("heliotrope"),
        "got: {as_bob}"
    );
}

#[test]
fn whoami_lists_the_other_people_this_device_knows() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = open_as(&dir, "alice");
    session.handle_utterance("/user bob");
    session.handle_utterance("/user alice");

    let reply = session.handle_utterance("/whoami").join("\n");
    assert!(reply.contains("also knows"), "got: {reply}");
    assert!(reply.contains("bob"), "got: {reply}");
}

#[test]
fn a_name_that_could_escape_its_own_directory_is_refused_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = open_as(&dir, "alice");

    let reply = session.handle_utterance("/user ../etc").join("\n");
    assert!(reply.contains("can't be a user name"), "got: {reply}");

    // A refused switch must leave the previous person exactly where they were.
    let who = session.handle_utterance("/whoami").join("\n");
    assert!(who.contains("alice"), "got: {who}");
}

#[test]
fn one_persons_saved_api_key_is_not_another_persons() {
    // The defect this exists for: one shared store under one device-derived key meant Bob's turn
    // could spend Alice's API credit. Proven behaviourally rather than by looking for a file --
    // the store is written lazily, so a filename proves nothing until a secret exists.
    let dir = tempfile::tempdir().unwrap();
    let mut session = open_as(&dir, "alice");

    session.handle_utterance("connect my openai account");
    assert!(
        session.awaiting_secret_input(),
        "the connect flow should be waiting for the key line"
    );
    let stored = session.handle_utterance("sk-alices-own-key").join("\n");
    assert!(stored.contains("Connected"), "got: {stored}");

    // Alice really has one now.
    let alice_sees = session
        .handle_utterance("/backend openai gpt-4o-mini")
        .join("\n");
    assert!(
        !alice_sees.contains("haven't connected"),
        "Alice really stored a key, got: {alice_sees}"
    );

    session.handle_utterance("/user bob");
    let bob_sees = session
        .handle_utterance("/backend openai gpt-4o-mini")
        .join("\n");
    assert!(
        bob_sees.contains("haven't connected"),
        "Bob must not inherit Alice's saved key, got: {bob_sees}"
    );

    // ...and Alice still has hers after the round trip.
    session.handle_utterance("/user alice");
    let alice_again = session
        .handle_utterance("/backend openai gpt-4o-mini")
        .join("\n");
    assert!(
        !alice_again.contains("haven't connected"),
        "Alice's own key must survive someone else using the device, got: {alice_again}"
    );
}

#[test]
fn a_session_opened_without_a_name_still_works_exactly_as_before() {
    // Every existing caller of `open` keeps working; the default person is an ordinary principal.
    let dir = tempfile::tempdir().unwrap();
    let mut session = ConsoleSession::open(dir.path()).expect("open");
    let reply = session.handle_utterance("/whoami").join("\n");
    assert!(reply.contains("default"), "got: {reply}");
}

#[test]
fn help_mentions_both_user_commands() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = open_as(&dir, "alice");
    let reply = session.handle_utterance("/help").join("\n");
    assert!(reply.contains("/whoami"), "got: {reply}");
    assert!(reply.contains("/user "), "got: {reply}");
}
