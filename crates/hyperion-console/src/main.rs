//! The real stdin/stdout loop around [`hyperion_console::ConsoleSession`] -- all the real logic
//! lives there and is tested directly; this binary is only real terminal I/O plumbing.

use std::io::{self, BufRead, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use hyperion_console::secret_input::RawEchoOff;
use hyperion_console::{ConsoleSession, TaskProgress};

const BANNER: &str = r#" _  ___   _____ ___ ___ ___ ___  _  _
| || \ \ / / _ \ __| _ \_ _/ _ \| \| |
| __ |\ V /|  _/ _||   /| | (_) | .` |
|_||_| |_| |_| |___|_|_\___\___/|_|\_|"#;

const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SPINNER_FRAME_INTERVAL: Duration = Duration::from_millis(80);

/// A real, live "this is still running" animation for whichever task names
/// [`TaskProgress::Starting`] just named -- a real background thread redraws the same terminal
/// line via `\r` every [`SPINNER_FRAME_INTERVAL`] until [`Self::stop`] is called. Only ever
/// constructed on a real interactive terminal (see this binary's own `is_terminal()` gate,
/// already established for the startup banner): the repeated `\r` redraws would corrupt a piped
/// or redirected caller's own output, which never sees this struct at all.
struct Spinner {
    running: Arc<AtomicBool>,
    label: String,
    handle: thread::JoinHandle<()>,
}

impl Spinner {
    fn start(task_names: &[String]) -> Self {
        let label = task_names.join(", ");
        let running = Arc::new(AtomicBool::new(true));
        let handle = {
            let running = running.clone();
            let label = label.clone();
            thread::spawn(move || {
                let mut frame = 0usize;
                while running.load(Ordering::Relaxed) {
                    print!(
                        "\r{} {label}...",
                        SPINNER_FRAMES[frame % SPINNER_FRAMES.len()]
                    );
                    let _ = io::stdout().flush();
                    frame += 1;
                    thread::sleep(SPINNER_FRAME_INTERVAL);
                }
            })
        };
        Spinner {
            running,
            label,
            handle,
        }
    }

    /// Stops the real background thread, then clears the spinner's own line (plain spaces + `\r`,
    /// not an ANSI clear sequence -- this crate has never assumed ANSI support anywhere else) so
    /// whatever prints next starts on a clean line.
    fn stop(self) {
        self.running.store(false, Ordering::Relaxed);
        let _ = self.handle.join();
        print!("\r{}\r", " ".repeat(self.label.chars().count() + 4));
        let _ = io::stdout().flush();
    }
}

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

    // Only for a real interactive terminal -- a screen reader, a pipe, or a redirected/scripted
    // caller gets straight to the one line that actually matters, not decorative noise before it.
    if io::stdout().is_terminal() {
        println!();
        println!("{BANNER}");
    }
    println!();
    println!("You ask. I understand.");
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

        // A real `Spinner` animates while a tick of a decomposed multi-task plan is still
        // blocked on its own real (potentially slow) capability dispatch -- see
        // `ConsoleSession::run_decomposed_plan`'s own doc comment for why `Starting` fires
        // *before* that blocking call, not only `Done` after it. This closure is the one real
        // place in this crate allowed to touch stdout directly mid-turn.
        let interactive = io::stdout().is_terminal();
        let mut spinner: Option<Spinner> = None;
        let output_lines =
            session.handle_utterance_with_progress(utterance, &mut |event| match event {
                TaskProgress::Starting(names) => {
                    if interactive && !names.is_empty() {
                        spinner = Some(Spinner::start(&names));
                    }
                }
                TaskProgress::Done(line) => {
                    if let Some(s) = spinner.take() {
                        s.stop();
                    }
                    println!("{line}");
                }
            });
        // A plan that errors or breaks out of its own tick loop before a real `Done` event
        // fires would otherwise leave the spinner animating forever -- stop it here too.
        if let Some(s) = spinner.take() {
            s.stop();
        }

        for output_line in output_lines {
            println!("{output_line}");
        }
        println!();
    }
}
