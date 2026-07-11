//! The real stdin/stdout loop around [`hyperion_console::ConsoleSession`] -- all the real logic
//! lives there and is tested directly; this binary is only real terminal I/O plumbing.

use std::io::{self, BufRead, Write};

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

        let mut line = String::new();
        match input.read_line(&mut line) {
            Ok(0) => break, // EOF: the terminal went away.
            Ok(_) => {}
            Err(_) => break,
        }

        let utterance = line.trim();
        if utterance.is_empty() {
            continue;
        }

        for output_line in session.handle_utterance(utterance) {
            println!("{output_line}");
        }
        println!();
    }
}
