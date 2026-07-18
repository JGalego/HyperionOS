//! `ConsoleSession`'s own previously-unnamed "no model selection persistence" gap: an explicit
//! `/backend`/`use backend` switch used to only ever last for the current process's own
//! lifetime, silently reverting to the candle-or-mock default on every restart. These tests
//! cover what's real and buildable without any extra Cargo feature -- the persistence mechanism
//! itself (a malformed or currently-unreachable persisted selection must never crash startup or
//! silently misreport what's actually active) -- see `candle_model_selection_persistence.rs` for
//! the full real round trip through an actually-successful switch, which needs `--features
//! candle` to have a second backend this build can actually reconnect to.

use hyperion_console::ConsoleSession;

fn model_selection_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("model_selection.json")
}

#[test]
fn a_fresh_install_with_no_persisted_selection_just_uses_the_real_default() {
    let dir = tempfile::tempdir().expect("create a real tempdir");
    assert!(!model_selection_path(dir.path()).exists());

    let session = ConsoleSession::open(dir.path()).expect("open a real ConsoleSession");
    assert_eq!(session.backend_label(), "mock");
}

#[test]
fn a_malformed_persisted_selection_never_crashes_startup() {
    let dir = tempfile::tempdir().expect("create a real tempdir");
    std::fs::create_dir_all(dir.path()).unwrap();
    std::fs::write(model_selection_path(dir.path()), "not real json at all").unwrap();

    let session = ConsoleSession::open(dir.path());
    assert!(
        session.is_ok(),
        "a hand-corrupted model_selection.json must degrade to the real default, never fail \
         to open the session at all"
    );
    assert_eq!(session.unwrap().backend_label(), "mock");
}

#[test]
fn a_well_formed_but_unrecognized_kind_is_treated_as_no_persisted_selection() {
    let dir = tempfile::tempdir().expect("create a real tempdir");
    std::fs::create_dir_all(dir.path()).unwrap();
    std::fs::write(
        model_selection_path(dir.path()),
        r#"{"kind": "some-future-backend-this-build-has-never-heard-of"}"#,
    )
    .unwrap();

    let session = ConsoleSession::open(dir.path()).expect("open a real ConsoleSession");
    assert_eq!(session.backend_label(), "mock");
}

/// A persisted selection this build genuinely can't reconnect to right now (no `openai-compat`
/// feature compiled in) must degrade to the real, already-working default rather than leaving
/// the session in a broken or inconsistent state.
#[test]
fn a_persisted_selection_this_build_cannot_reach_falls_back_to_the_real_default() {
    let dir = tempfile::tempdir().expect("create a real tempdir");
    std::fs::create_dir_all(dir.path()).unwrap();
    std::fs::write(
        model_selection_path(dir.path()),
        r#"{"kind": "engine", "engine": "ollama", "base_url": "http://localhost:11434/v1", "model": "llama3"}"#,
    )
    .unwrap();

    let session = ConsoleSession::open(dir.path()).expect("open a real ConsoleSession");
    assert_eq!(
        session.backend_label(),
        "mock",
        "this test binary has no openai-compat feature compiled in, so restoring the persisted \
         engine selection must fail gracefully and leave the session on its real, working \
         default rather than an unreachable backend"
    );
}

/// A persisted selection identical to what would have been chosen anyway is a real no-op --
/// `ConsoleSession::open` must not attempt (and can't fail) a redundant reconnect.
#[test]
fn a_persisted_selection_matching_the_real_default_is_a_silent_no_op() {
    let dir = tempfile::tempdir().expect("create a real tempdir");
    std::fs::create_dir_all(dir.path()).unwrap();
    std::fs::write(model_selection_path(dir.path()), r#"{"kind": "mock"}"#).unwrap();

    let session = ConsoleSession::open(dir.path()).expect("open a real ConsoleSession");
    assert_eq!(session.backend_label(), "mock");
}

/// A failed `/backend` switch attempt must never overwrite a real, previously-persisted
/// selection with a broken one -- only a genuinely successful switch is ever written.
#[test]
fn a_failed_backend_switch_never_persists_anything() {
    let dir = tempfile::tempdir().expect("create a real tempdir");
    let mut session = ConsoleSession::open(dir.path()).expect("open a real ConsoleSession");
    assert!(!model_selection_path(dir.path()).exists());

    let reply = session.handle_utterance("/backend candle").join("\n");
    assert!(
        reply.contains("couldn't switch"),
        "this test binary has no candle feature compiled in, so this switch must fail, got: \
         {reply:?}"
    );
    assert!(
        !model_selection_path(dir.path()).exists(),
        "a failed switch must never create a persisted selection file"
    );
    assert_eq!(session.backend_label(), "mock");
}
