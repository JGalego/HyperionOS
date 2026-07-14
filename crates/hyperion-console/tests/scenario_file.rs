//! Real, end-to-end coverage of `hyperion-console <SCENARIO>` (docs/999-usage-scenarios.md's "how to run
//! a scenario" section) -- spawns the actual compiled binary via `CARGO_BIN_EXE_hyperion-console`
//! (set automatically by cargo for an integration test of a crate that also builds that binary),
//! since this behavior lives in `main.rs` itself, deliberately kept out of the `ConsoleSession`
//! library API this crate's other tests exercise directly.

use std::io::Write;
use std::process::Command;

fn run_scenario(scenario_contents: &str, extra_env: &[(&str, &str)]) -> (String, String, bool) {
    let dir = tempfile::tempdir().expect("create a real tempdir for this test");
    let scenario_path = dir.path().join("scenario.txt");
    std::fs::File::create(&scenario_path)
        .and_then(|mut f| f.write_all(scenario_contents.as_bytes()))
        .expect("write a real scenario file");

    let mut command = Command::new(env!("CARGO_BIN_EXE_hyperion-console"));
    command
        .arg(&scenario_path)
        .env("HYPERION_CONSOLE_DATA_DIR", dir.path().join("data"));
    for (key, value) in extra_env {
        command.env(key, value);
    }

    let output = command
        .output()
        .expect("spawn the real compiled hyperion-console binary");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    )
}

/// The whole point of a scenario file over bare stdin redirection: a real, readable transcript --
/// each utterance echoed as `"> {utterance}"` immediately before its own real response, so the
/// output makes sense read top to bottom with no other context.
#[test]
fn a_scenario_file_echoes_each_utterance_before_its_own_real_response() {
    let (stdout, stderr, success) = run_scenario("hello there\nwhat is my name\n", &[]);

    assert!(success, "expected a clean exit, stderr: {stderr:?}");
    assert!(
        stdout.contains("> hello there"),
        "expected the first utterance echoed verbatim, got: {stdout:?}"
    );
    assert!(
        stdout.contains("> what is my name"),
        "expected the second utterance echoed verbatim, got: {stdout:?}"
    );
    assert!(
        stdout.contains("echo: hello there"),
        "expected a real response to the first turn, got: {stdout:?}"
    );
}

/// A blank line or a `#`-prefixed comment documents/spaces out a scenario file without becoming
/// a real (nonsensical, empty) utterance -- the file is meant to be readable and checked in, not
/// a bare list of turns.
#[test]
fn blank_lines_and_comments_are_skipped_not_sent_as_real_utterances() {
    let (stdout, _stderr, success) = run_scenario(
        "# this is a demo scenario\n\nhello there\n# another comment\n",
        &[],
    );

    assert!(success);
    assert!(
        !stdout.contains("this is a demo scenario"),
        "a comment line must never reach the real session, got: {stdout:?}"
    );
    assert!(!stdout.contains("another comment"), "got: {stdout:?}");
    assert!(
        stdout.contains("> hello there"),
        "the one real utterance must still go through, got: {stdout:?}"
    );
}

/// The real reason scenario files need this at all: a checked-in file can reference
/// `$MY_API_KEY` by name (never a real literal secret) and still drive a real connection when
/// `source .env` has actually set it -- the same interpolation a shell already did for the
/// `printf ... "$OPENAI_API_KEY"` pattern this replaces.
#[test]
fn a_dollar_reference_expands_against_the_real_process_environment() {
    let (stdout, _stderr, success) = run_scenario(
        "greet me with $HYPERION_SCENARIO_TEST_VAR\n",
        &[("HYPERION_SCENARIO_TEST_VAR", "value-from-env")],
    );

    assert!(success);
    assert!(
        stdout.contains("greet me with value-from-env"),
        "expected the real env var's value substituted in, got: {stdout:?}"
    );
    assert!(
        !stdout.contains("HYPERION_SCENARIO_TEST_VAR"),
        "the raw reference must not survive expansion, got: {stdout:?}"
    );
}

/// An unset reference must be left exactly as written, not silently blanked -- a silently empty
/// secret would fail downstream with a confusing error instead of an honest "this var isn't set."
#[test]
fn an_unset_dollar_reference_is_left_untouched() {
    let (stdout, _stderr, success) =
        run_scenario("this has a $HYPERION_DEFINITELY_UNSET_VAR reference\n", &[]);

    assert!(success);
    assert!(
        stdout.contains("$HYPERION_DEFINITELY_UNSET_VAR"),
        "expected the unset reference preserved literally, got: {stdout:?}"
    );
}

/// A real, previously-shipped risk this guards against: a scenario file's API-key line must never
/// be echoed to stdout in the clear, exactly as a real interactive terminal's own
/// `RawEchoOff`-backed "connect" flow already never echoes a real typed key.
#[test]
fn a_pasted_api_key_line_is_redacted_in_the_echoed_transcript() {
    let (stdout, _stderr, success) = run_scenario(
        "connect my openai account\n$HYPERION_SCENARIO_FAKE_KEY\n",
        &[("HYPERION_SCENARIO_FAKE_KEY", "sk-not-a-real-secret-12345")],
    );

    assert!(success);
    assert!(
        stdout.contains("[key redacted]"),
        "expected the key line's echo to be redacted, got: {stdout:?}"
    );
    assert!(
        !stdout.contains("sk-not-a-real-secret-12345"),
        "the real secret must never appear in the echoed transcript, got: {stdout:?}"
    );
    assert!(
        stdout.contains("Connected"),
        "the real key must still have been used to actually connect, got: {stdout:?}"
    );
}

/// The binary must exit cleanly after a scenario file's last line rather than blocking on stdin
/// waiting for more input that will never come -- `Command::output()` above would itself hang
/// (and this test would time out) if it didn't.
#[test]
fn the_binary_exits_after_the_scenario_file_ends_rather_than_blocking_on_stdin() {
    let (_stdout, _stderr, success) = run_scenario("hello there\n", &[]);
    assert!(success, "expected a clean, non-hanging exit");
}
