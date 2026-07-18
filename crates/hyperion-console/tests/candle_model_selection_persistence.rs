//! The full real round trip `model_selection_persistence.rs` can't exercise without a second
//! backend this build can actually reconnect to: switch away from the real, network-loaded
//! Candle default to Mock, confirm the switch really persisted, then reopen a brand-new
//! `ConsoleSession` against the same real data directory and confirm it restores Mock instead of
//! re-loading Candle -- a real, deliberate choice surviving a real restart, not just the current
//! process's own lifetime.
//!
//! `#[cfg(feature = "candle")]`-gated, like `hyperion-ai-runtime`'s own `candle_inference.rs`:
//! this downloads a real model on first run (cached afterward), so it does not run as part of
//! the default `cargo test --workspace` gate. Invoke explicitly with `cargo test -p
//! hyperion-console --features candle --test candle_model_selection_persistence`.

#![cfg(feature = "candle")]

use hyperion_console::ConsoleSession;

fn model_selection_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("model_selection.json")
}

#[test]
fn a_real_explicit_switch_survives_a_real_restart() {
    let dir = tempfile::tempdir().expect("create a real tempdir");

    let mut session = ConsoleSession::open(dir.path()).expect("open a real ConsoleSession");
    assert_eq!(
        session.backend_label(),
        "candle",
        "a candle-featured build must really load the real Candle backend by default"
    );
    assert!(!model_selection_path(dir.path()).exists());

    let reply = session.handle_utterance("/backend mock").join("\n");
    assert!(
        reply.contains("Switched to the mock backend"),
        "got: {reply:?}"
    );
    assert_eq!(session.backend_label(), "mock");

    let persisted = std::fs::read_to_string(model_selection_path(dir.path()))
        .expect("a real, successful switch must really persist to model_selection.json");
    let parsed: serde_json::Value = serde_json::from_str(&persisted).unwrap();
    assert_eq!(parsed["kind"], "mock");

    drop(session);

    let restarted = ConsoleSession::open(dir.path()).expect("open a second real ConsoleSession");
    assert_eq!(
        restarted.backend_label(),
        "mock",
        "a real restart must restore the real, previously-persisted explicit choice, not \
         silently revert to the candle-or-mock default"
    );
}
