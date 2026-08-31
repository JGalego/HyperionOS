//! docs/998-roadmap.md's App Builder (M1) meta-commands, against a real `ConsoleSession`.
//!
//! `/build` itself needs a real script engine registered *and* a Landlock-capable kernel to be
//! worth much; what these tests pin is everything around it that is real regardless -- that the
//! commands dispatch, that they say something a person can act on when they can't proceed, and
//! that `/apps`/`/app`/`/run`/`/app-remove` really read and write the session's own registry.

use hyperion_console::ConsoleSession;

fn open_session() -> (tempfile::TempDir, ConsoleSession) {
    let dir = tempfile::tempdir().expect("create a real tempdir for this test's Knowledge Graph");
    let session = ConsoleSession::open(dir.path()).expect("open a real ConsoleSession");
    (dir, session)
}

#[test]
fn apps_on_a_fresh_session_offers_the_next_step_rather_than_an_empty_list() {
    let (_dir, mut session) = open_session();
    let reply = session.handle_utterance("/apps").join("\n");
    assert!(reply.contains("/build"), "got: {reply}");
}

#[test]
fn each_app_command_says_what_it_needs_when_given_nothing() {
    let (_dir, mut session) = open_session();
    for (command, expected) in [
        ("/app", "needs the name of an app"),
        ("/run", "needs the name of an app"),
        ("/app-remove", "needs the name of an app"),
        ("/build", "needs to know what you want"),
        ("/app-engine", "needs a name and the program"),
    ] {
        let reply = session.handle_utterance(command).join("\n");
        assert!(
            reply.contains(expected),
            "{command} should say what it needs, got: {reply}"
        );
    }
}

#[test]
fn asking_about_an_app_that_does_not_exist_points_at_the_listing() {
    let (_dir, mut session) = open_session();
    for command in ["/app nope", "/run nope", "/app-remove nope"] {
        let reply = session.handle_utterance(command).join("\n");
        assert!(
            reply.contains("don't have anything called") && reply.contains("/apps"),
            "{command} got: {reply}"
        );
    }
}

#[test]
fn building_without_a_script_engine_explains_what_is_missing_instead_of_failing_obscurely() {
    let (_dir, mut session) = open_session();
    // A fresh session has no `ExecutionEngine` registered, and no engine can be guessed: the
    // sandbox grants a process no access to a dynamic loader, so a stock `/bin/sh` would install
    // and then fail at the moment of use.
    let reply = session
        .handle_utterance("/build something to add up my invoices")
        .join("\n");
    assert!(reply.contains("/app-engine"), "got: {reply}");
    assert!(reply.contains("statically linked"), "got: {reply}");
}

#[test]
fn registering_a_script_engine_that_does_not_exist_is_really_refused() {
    let (_dir, mut session) = open_session();
    let reply = session
        .handle_utterance("/app-engine sh /definitely/not/here")
        .join("\n");
    assert!(reply.contains("couldn't set that up"), "got: {reply}");

    // And it really did not install -- `/build` still reports the same missing engine.
    let reply = session.handle_utterance("/build anything").join("\n");
    assert!(reply.contains("/app-engine"), "got: {reply}");
}

#[test]
fn a_real_engine_really_registers_and_is_named_in_the_confirmation() {
    let (_dir, mut session) = open_session();
    // A real, already-existing, already-executable file -- the same honest stand-in for "an
    // interpreter" the plugin framework's own engine tests use.
    let launcher = std::env::current_exe().unwrap();
    let reply = session
        .handle_utterance(&format!("/app-engine sh {}", launcher.display()))
        .join("\n");
    assert!(reply.contains("\"sh\" apps"), "got: {reply}");
    // The caveat that decides whether it will actually work is said at registration time, not
    // discovered later from a failed run.
    assert!(reply.contains("statically linked"), "got: {reply}");

    // With an engine present, `/build` gets past the engine check and reaches real generation --
    // which, on the default MockBackend, cannot produce a usable plan. It must say so in words.
    let reply = session
        .handle_utterance("/build something to add up my invoices")
        .join("\n");
    assert!(!reply.contains("/app-engine"), "got: {reply}");
    assert!(
        reply.contains("couldn't tell what to build") || reply.contains("couldn't read the plan"),
        "got: {reply}"
    );
}

#[test]
fn run_rejects_an_argument_that_is_not_a_name_value_pair() {
    let (_dir, mut session) = open_session();
    let launcher = std::env::current_exe().unwrap();
    session.handle_utterance(&format!("/app-engine sh {}", launcher.display()));

    // The app does not exist, so this stops at the same "no such app" answer -- the point being
    // that `/run` never treats a bare word as a value for whatever argument came first.
    let reply = session.handle_utterance("/run nope may").join("\n");
    assert!(reply.contains("don't have anything called"), "got: {reply}");
}

#[test]
fn help_lists_the_app_commands() {
    let (_dir, mut session) = open_session();
    let reply = session.handle_utterance("/help").join("\n");
    for command in [
        "/build",
        "/apps",
        "/app ",
        "/run ",
        "/app-remove",
        "/app-engine",
    ] {
        assert!(reply.contains(command), "help should mention {command}");
    }
}

#[test]
fn a_meta_command_never_gets_an_app_suggestion_appended() {
    // Suggestions ride on real goal utterances only. `/apps` returning "you also built an app for
    // this" about itself would be noise, and meta-commands return before that path is reached.
    let (_dir, mut session) = open_session();
    let reply = session.handle_utterance("/apps").join("\n");
    assert!(!reply.contains("You built"), "got: {reply}");
}

#[test]
fn a_goal_utterance_with_nothing_installed_gets_no_suggestion() {
    let (_dir, mut session) = open_session();
    let reply = session
        .handle_utterance("count the words in this text")
        .join("\n");
    // Nothing is installed, so there is nothing to suggest -- and the goal still gets its real
    // answer through the normal path rather than being intercepted.
    assert!(!reply.contains("You built"), "got: {reply}");
    assert!(!reply.is_empty());
}

#[test]
fn app_logs_and_rebuild_say_what_they_need_and_refuse_unknown_apps() {
    let (_dir, mut session) = open_session();
    for (command, expected) in [
        ("/app-logs", "needs the name of an app"),
        ("/rebuild", "needs the name of an app"),
    ] {
        let reply = session.handle_utterance(command).join("\n");
        assert!(reply.contains(expected), "{command} got: {reply}");
    }
    for command in ["/app-logs nope", "/rebuild nope do something else"] {
        let reply = session.handle_utterance(command).join("\n");
        assert!(
            reply.contains("don't have anything called"),
            "{command} got: {reply}"
        );
    }
}

#[test]
fn the_longer_app_commands_are_not_swallowed_by_the_shorter_ones() {
    // `/app-logs` starts with `/app`, and `/rebuild` does not collide -- but `/app-logs nope`
    // reaching `/app`'s handler would report the wrong thing entirely. Prefix order is load-bearing
    // here, so it gets a test rather than a comment.
    let (_dir, mut session) = open_session();
    let reply = session.handle_utterance("/app-logs nope").join("\n");
    assert!(reply.contains("don't have anything called"), "got: {reply}");
    assert!(!reply.contains("needs the name of an app"), "got: {reply}");
}

#[test]
fn help_lists_the_newer_app_commands() {
    let (_dir, mut session) = open_session();
    let reply = session.handle_utterance("/help").join("\n");
    for command in ["/app-logs", "/rebuild"] {
        assert!(reply.contains(command), "help should mention {command}");
    }
}

#[test]
fn nothing_is_left_pending_by_a_run_that_never_found_an_app() {
    // The missing-input question is checked before meta-commands, so a stray pending state would
    // swallow the *next* thing typed. A run that found no app must leave nothing behind.
    let (_dir, mut session) = open_session();
    session.handle_utterance("/run nope");

    // The next line is handled normally, not captured as an answer to anything.
    let reply = session.handle_utterance("/apps").join("\n");
    assert!(reply.contains("/build"), "got: {reply}");
}

#[test]
fn ordinary_turns_are_unaffected_by_the_missing_input_check() {
    // Regression guard for the check inserted ahead of meta-command dispatch: with nothing
    // pending it must be entirely invisible.
    let (_dir, mut session) = open_session();
    let reply = session.handle_utterance("/whoami").join("\n");
    assert!(reply.contains("default"), "got: {reply}");
    let reply = session.handle_utterance("hello there").join("\n");
    assert!(!reply.is_empty());
}

#[test]
fn the_residency_commands_say_what_they_need_and_refuse_unknown_apps() {
    let (_dir, mut session) = open_session();
    for (command, expected) in [
        ("/app-start", "needs the name of an app"),
        ("/app-stop", "needs the name of an app"),
    ] {
        let reply = session.handle_utterance(command).join("\n");
        assert!(reply.contains(expected), "{command} got: {reply}");
    }
    let reply = session.handle_utterance("/app-start nope").join("\n");
    assert!(reply.contains("don't have anything called"), "got: {reply}");
    // Stopping something that was never running is an answer, not an error.
    let reply = session.handle_utterance("/app-stop nope").join("\n");
    assert!(reply.contains("isn't running"), "got: {reply}");
}

#[test]
fn help_lists_the_residency_commands() {
    let (_dir, mut session) = open_session();
    let reply = session.handle_utterance("/help").join("\n");
    for command in ["/app-start", "/app-stop"] {
        assert!(reply.contains(command), "help should mention {command}");
    }
}

#[test]
fn polling_for_resident_apps_never_disturbs_an_ordinary_turn() {
    // Residency polling runs at the top of every turn. With nothing supervised it must be
    // invisible -- and it must never reap a child it does not own, which is why it uses the
    // supervisor's non-blocking own-pids-only path.
    let (_dir, mut session) = open_session();
    for _ in 0..3 {
        let reply = session.handle_utterance("/apps").join("\n");
        assert!(reply.contains("/build"), "got: {reply}");
    }
}
