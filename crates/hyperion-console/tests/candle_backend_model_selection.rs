//! `/backend candle <model>` -- this crate's own previously-named "no way to pick a model via
//! the console" gap: a bare `/backend candle` always loaded the same built-in default, with no
//! way to select a different real catalog entry or a real Hugging Face Hub repo directly.
//!
//! `#[cfg(feature = "candle")]`-gated like `candle_model_selection_persistence.rs`: this downloads
//! real models on first run (cached afterward), so it does not run as part of the default `cargo
//! test --workspace` gate. Invoke explicitly with `cargo test -p hyperion-console --features
//! candle --test candle_backend_model_selection`.

#![cfg(feature = "candle")]

use hyperion_console::ConsoleSession;

#[test]
fn backend_candle_with_a_real_catalog_name_really_switches_to_that_model() {
    let dir = tempfile::tempdir().expect("create a real tempdir");
    let mut session = ConsoleSession::open(dir.path()).expect("open a real ConsoleSession");
    assert_eq!(session.backend_label(), "candle");

    let reply = session
        .handle_utterance("/backend candle stories15m-gguf")
        .join("\n");
    assert!(reply.contains("Switched to the candle"), "got: {reply:?}");
    assert_eq!(
        session.backend_label(),
        "candle (model \"stories15m-gguf\")"
    );
}

#[test]
fn backend_candle_with_an_unknown_model_fails_gracefully_and_keeps_the_current_backend() {
    let dir = tempfile::tempdir().expect("create a real tempdir");
    let mut session = ConsoleSession::open(dir.path()).expect("open a real ConsoleSession");
    assert_eq!(session.backend_label(), "candle");

    let reply = session
        .handle_utterance("/backend candle no-such-model-anyone-has-ever-heard-of")
        .join("\n");
    assert!(reply.contains("couldn't switch"), "got: {reply:?}");
    assert_eq!(
        session.backend_label(),
        "candle",
        "a failed model switch must never leave the session on a broken backend"
    );
}

#[test]
fn backend_candle_with_a_real_hf_triple_really_switches() {
    let dir = tempfile::tempdir().expect("create a real tempdir");
    let mut session = ConsoleSession::open(dir.path()).expect("open a real ConsoleSession");

    let reply = session
        .handle_utterance(
            "/backend candle klosax/tinyllamas-stories-gguf/tinyllamas-stories-15m-f32.gguf",
        )
        .join("\n");
    assert!(reply.contains("Switched to the candle"), "got: {reply:?}");
}
