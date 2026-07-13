//! The real stdin/stdout loop around [`hyperion_console::ConsoleSession`] -- all the real logic
//! lives there and is tested directly; this binary is only real terminal I/O plumbing.

use std::io::{self, BufRead, Write};

use hyperion_console::secret_input::RawEchoOff;
use hyperion_console::ConsoleSession;

fn main() {
    let data_dir = std::env::var("HYPERION_CONSOLE_DATA_DIR")
        .unwrap_or_else(|_| std::env::temp_dir().display().to_string());

    let mut session = match ConsoleSession::open(&data_dir) {
        Ok(session) => session,
        Err(e) => {
            eprintln!(
                "I couldn't start up: my own Knowledge Graph at {data_dir:?} failed to open \
                 ({e})."
            );
            std::process::exit(1);
        }
    };

    println!();
    println!("Hyperion -- tell me what you'd like to do.");
    println!();

    let stdin = io::stdin();
    let mut input = stdin.lock();
    loop {
        print!("> ");
        if io::stdout().flush().is_err() {
            break;
        }

        // A "connect my <provider> account" flow's follow-up API-key line must not be echoed to
        // the terminal or left sitting in scrollback -- checked before the real read, since
        // ECHO has to be off *during* it, not after `handle_utterance` already has the line.
        let awaiting_secret = session.awaiting_secret_input();
        let mut line = String::new();
        let read_result = if awaiting_secret {
            let _guard = RawEchoOff::disable();
            let result = input.read_line(&mut line);
            println!(); // ECHO being off also swallowed the Enter keystroke's own visible newline.
            result
        } else {
            input.read_line(&mut line)
        };
        match read_result {
            Ok(0) => break, // EOF: the terminal went away.
            Ok(_) => {}
            Err(_) => break,
        }

        let utterance = line.trim();
        // An empty line while awaiting a secret is itself a real, legitimate answer (cancel
        // connecting) that `ConsoleSession::handle_utterance` must still see -- only a genuinely
        // empty *goal* utterance gets silently skipped.
        if utterance.is_empty() && !awaiting_secret {
            continue;
        }

        for output_line in session.handle_utterance(utterance) {
            println!("{output_line}");
        }
        println!();
    }
}
