//! Terminal color and small status symbols for this binary's own real stdout -- applied only
//! here, at the point of printing, never inside `ConsoleSession`'s returned `Vec<String>`. That
//! boundary matters: the exact same strings also flow through `/mcp-server`/`/a2a-server`'s real
//! JSON-RPC responses (a raw ANSI escape in a JSON string body would be a real protocol bug, not
//! a cosmetic one) and through this crate's own tests, which assert on plain text. Every rule
//! below only wraps an already-real, already-established phrase this codebase already prints --
//! it never invents new wording, and [`enabled`] makes the whole thing a no-op (identical bytes
//! to before this module existed) for anything that isn't a real interactive terminal: a pipe, a
//! redirect, `CARGO_BIN_EXE_hyperion-console`-spawned test output, or `NO_COLOR` set
//! (<https://no-color.org>). CLAUDE.md's accessibility-first stance, concretely: color and the
//! `✓`/`⚠`/`✗` glyphs are always redundant with words already on the line, never the only signal.
//!
//! The palette is website/app/globals.css's own real `.dark` block -- the same real colors
//! assets/generate-banner.py and assets/vhs/*.tape already use, not invented here.

use std::io::IsTerminal;

const ACCENT: &str = "\x1b[38;2;217;165;74m"; // #d9a54a
const GREEN: &str = "\x1b[38;2;143;174;106m"; // #8fae6a
const AMBER: &str = "\x1b[38;2;230;187;110m"; // #e6bb6e
const RED: &str = "\x1b[38;2;176;112;95m"; // #b0705f
const RESET: &str = "\x1b[0m";

/// Whether this real process should actually emit color/symbols right now. Checked fresh at each
/// call site rather than cached once, matching how `io::stdout().is_terminal()` is already
/// checked fresh elsewhere in this binary (the banner, the spinner).
pub fn enabled() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

/// Colorizes the real `"> "` prompt -- both the interactive read prompt and a scenario file's own
/// echoed-utterance prefix (`"> {utterance}"`) -- so a real terminal reads it as unambiguously the
/// console's own input marker, distinct from the response text that follows.
pub fn prompt(text: &str) -> String {
    if enabled() {
        format!("{ACCENT}{text}{RESET}")
    } else {
        text.to_string()
    }
}

/// Colorizes the real startup banner -- the same accent this crate's own `/backend`, `/graph`,
/// and every other real prompt marker uses, so the one-time banner and the ongoing session read
/// as the same visual identity.
pub fn banner(text: &str) -> String {
    prompt(text)
}

/// Colorizes one already-rendered response line, by real, already-established phrasing this
/// crate's own text already uses -- never changes the words, only wraps them (plus a redundant
/// leading glyph), so a line means exactly the same thing whether or not color is actually on.
/// Returns the line unchanged for anything that doesn't match a known shape, which is most lines:
/// this is deliberately conservative rather than guessing at a real model's own free-form prose.
pub fn status_line(line: &str) -> String {
    if !enabled() {
        return line.to_string();
    }
    if is_success(line) {
        format!("{GREEN}\u{2713} {line}{RESET}")
    } else if line.starts_with("warning:") || ends_with_status(line, "Blocked") {
        format!("{AMBER}\u{26a0} {line}{RESET}")
    } else if is_failure(line) {
        format!("{RED}\u{2717} {line}{RESET}")
    } else {
        line.to_string()
    }
}

/// A real decomposed-plan tick's own progress line is always exactly `"  {task}: {Status:?}"`
/// (`ConsoleSession::drive_ticks_to_completion`) -- anchoring on the two-space indent plus a
/// trailing status word (not a bare substring search) keeps this from ever matching a real
/// model's own generated prose, which never happens to start a line with exactly two spaces and
/// end it with a bare status word.
fn ends_with_status(line: &str, status: &str) -> bool {
    line.starts_with("  ") && line.trim_end().ends_with(&format!(": {status}"))
}

fn is_success(line: &str) -> bool {
    line.starts_with("status: done --")
        || line.contains(": Done --")
        || line.starts_with("Switched to the ")
        || line.starts_with("Connected (")
        || ends_with_status(line, "Done")
}

fn is_failure(line: &str) -> bool {
    line.starts_with("I couldn't")
        || line.starts_with("I don't know")
        || line.starts_with("I don't recognize")
        || line.starts_with("I understood that, but couldn't")
        || line.contains(": Failed --")
        || ends_with_status(line, "Failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    // `enabled()` is false under `cargo test`'s own captured (piped, non-tty) stdout, so these
    // exercise the classifier logic itself via the same code path `status_line` would use if it
    // ever were enabled, rather than asserting on literal escape bytes that never appear here.

    #[test]
    fn a_decomposed_tasks_done_line_is_classified_as_success() {
        assert!(is_success("  market_research: Done"));
        assert!(!is_failure("  market_research: Done"));
    }

    #[test]
    fn a_decomposed_tasks_failed_line_is_classified_as_failure() {
        assert!(is_failure("  market_research: Failed"));
        assert!(!is_success("  market_research: Failed"));
    }

    #[test]
    fn a_generic_goals_status_line_is_classified_as_success() {
        assert!(is_success("status: done -- [mock model 1] echo: hello"));
    }

    #[test]
    fn a_real_models_prose_mentioning_the_word_done_is_not_misclassified() {
        assert!(!is_success(
            "The Roman Empire was done building its aqueducts by 100 AD."
        ));
        assert!(!is_failure(
            "The Roman Empire was done building its aqueducts by 100 AD."
        ));
    }

    #[test]
    fn an_honest_error_message_is_classified_as_failure() {
        assert!(is_failure(
            "I couldn't switch: this build wasn't compiled with real inference support."
        ));
        assert!(is_failure(
            "I don't recognize \"/nonexistent\" as a command -- try \"/help\"."
        ));
    }

    #[test]
    fn status_line_is_a_no_op_when_color_is_disabled() {
        // enabled() is always false here (no real tty under `cargo test`), so this is the actual
        // behavior every integration test spawning the real binary already relies on.
        assert_eq!(
            status_line("  market_research: Done"),
            "  market_research: Done"
        );
    }
}
