//! The real stdin/stdout loop around [`hyperion_console::ConsoleSession`] -- all the real logic
//! lives there and is tested directly; this binary is only real terminal I/O plumbing (plus, now,
//! the real MCP/A2A server and client transports -- see [`mcp`]/[`a2a`]/[`http_server`]/
//! [`http_client`], docs/998-roadmap.md's Social pillar), and real mDNS/DNS-SD advertise+discover
//! (see [`discovery`], same Social pillar's own next-named slice).

mod a2a;
mod color;
mod discovery;
mod http_client;
mod http_server;
mod mcp;

use std::io::{self, BufRead, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
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

/// Arbitrary, unregistered/user ports (RFC 6335) -- just real, fixed defaults so `/mcp-server`/
/// `/a2a-server` work with no argument; either can still be given an explicit port.
const DEFAULT_MCP_PORT: u16 = 8765;
const DEFAULT_A2A_PORT: u16 = 8766;

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
                        color::prompt(SPINNER_FRAMES[frame % SPINNER_FRAMES.len()])
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

    let session = match ConsoleSession::open(&data_dir) {
        Ok(session) => session,
        Err(e) => {
            eprintln!(
                "I couldn't start up: my own Knowledge Graph at {data_dir:?} failed to open \
                 ({e})."
            );
            std::process::exit(1);
        }
    };
    // Shared, not owned outright, from here on: `/mcp-server`/`/a2a-server` spawn real background
    // threads that need their own handle to the exact same live session -- a real MCP/A2A tool
    // call and a real typed utterance must see and affect the same conversation, not two
    // divergent copies.
    let session = Arc::new(Mutex::new(session));

    // A bare positional argument is a scenario file (docs/999-usage-scenarios.md's own "how to run a
    // scenario" section) -- `source .env && hyperion-console scenarios/foo.txt` in place of the
    // fragile `printf '%s\n' "..." | hyperion-console` pattern that pattern's own file had no
    // real way to check in with secrets still injected only at run time.
    if let Some(scenario_path) = std::env::args().nth(1) {
        run_scenario_file(&scenario_path, &session, &data_dir);
        return;
    }

    run_interactive(&session, &data_dir);
}

/// What a real, typed line meant to control the *process* itself, rather than being a normal
/// utterance headed for [`ConsoleSession::handle_utterance`] -- checked before that call in both
/// [`run_interactive`] and [`run_scenario_file`], the same "meta-command tier, ahead of the real
/// goal pipeline" precedent `ConsoleSession::handle_meta_command` itself already established one
/// layer down. Lives here, not there, because starting a background server needs a real
/// `Arc<Mutex<ConsoleSession>>` handle to hand a thread -- `ConsoleSession`'s own methods only
/// ever see `&mut self`, never a handle to share.
enum ControlOutcome {
    /// Not a control command at all -- proceed to the real utterance pipeline as normal.
    NotRecognized,
    /// A control command that's already done its real work; print these lines and keep going.
    Handled(Vec<String>),
    /// The process should end now (real user input at a real `/standby` was just given).
    Exit,
}

/// A real, bounded LAN scan's own default patience -- long enough for real mDNS responders to
/// really answer (multicast queries/responses aren't instantaneous), short enough that
/// `/mcp-discover`/`/a2a-discover` still feels like a command, not a hang.
const DEFAULT_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(3);

/// Recognizes `/mcp-server [port]`, `/a2a-server [port]`, `/mcp-discover [seconds]`,
/// `/a2a-discover [seconds]`, `/standby`, `/mcp-call`, `/a2a-call`, `/trust list`, and
/// `/trust forget <peer>` -- see each real handler's own doc comment. Returns
/// [`ControlOutcome::NotRecognized`] for everything else, so the normal utterance pipeline runs
/// unchanged.
fn try_control_command(
    trimmed: &str,
    session: &Arc<Mutex<ConsoleSession>>,
    data_dir: &str,
) -> ControlOutcome {
    let lower = trimmed.to_ascii_lowercase();

    if let Some(rest) = lower.strip_prefix("/mcp-server") {
        return start_mcp_server(session, rest.trim());
    }
    if let Some(rest) = lower.strip_prefix("/a2a-server") {
        return start_a2a_server(session, rest.trim());
    }
    if let Some(rest) = lower.strip_prefix("/mcp-discover") {
        return ControlOutcome::Handled(vec![discover_peers(
            discovery::MCP_SERVICE_TYPE,
            rest.trim(),
        )]);
    }
    if let Some(rest) = lower.strip_prefix("/a2a-discover") {
        return ControlOutcome::Handled(vec![discover_peers(
            discovery::A2A_SERVICE_TYPE,
            rest.trim(),
        )]);
    }
    if lower == "/standby" {
        return standby();
    }
    if let Some(rest) = trimmed.strip_prefix("/mcp-call ") {
        return ControlOutcome::Handled(vec![mcp_call(rest.trim())]);
    }
    if let Some(rest) = trimmed.strip_prefix("/a2a-call ") {
        return ControlOutcome::Handled(vec![a2a_call(rest.trim(), data_dir)]);
    }
    if let Some(rest) = lower.strip_prefix("/trust") {
        return ControlOutcome::Handled(vec![trust_command(rest.trim(), data_dir)]);
    }

    ControlOutcome::NotRecognized
}

fn peer_trust_path(data_dir: &str) -> std::path::PathBuf {
    std::path::Path::new(data_dir).join("peer_trust.json")
}

/// `/trust list` -- lists every real, currently-trusted `(peer, key)` pair (see
/// [`hyperion_console::peer_trust::PeerTrustStore::trusted_peers`]). `/trust forget <peer>` --
/// the user's own real, explicit override once a key-mismatch warning has been investigated
/// (see [`hyperion_console::peer_trust::PeerTrustStore::forget`]'s own doc comment on why this
/// exists at all).
fn trust_command(rest: &str, data_dir: &str) -> String {
    let mut store = match hyperion_console::peer_trust::PeerTrustStore::open_or_create(
        peer_trust_path(data_dir),
    ) {
        Ok(store) => store,
        Err(e) => return format!("I couldn't open the real peer trust store: {e}"),
    };

    if rest.is_empty() || rest == "list" {
        let peers = store.trusted_peers();
        if peers.is_empty() {
            return "No peers trusted yet.".to_string();
        }
        let mut lines = vec!["Trusted peers:".to_string()];
        for (peer_id, key_hex) in peers {
            lines.push(format!("  {peer_id} -- {key_hex}"));
        }
        return lines.join("\n");
    }

    if let Some(peer_id) = rest.strip_prefix("forget ") {
        return match store.forget(peer_id.trim()) {
            Ok(true) => format!("Forgot {peer_id}'s trusted identity."),
            Ok(false) => format!("{peer_id} wasn't trusted to begin with."),
            Err(e) => format!("I couldn't update the real peer trust store: {e}"),
        };
    }

    format!("\"/trust {rest}\" isn't a command I know -- try \"/trust list\" or \"/trust forget <peer>\".")
}

/// Real background advertisement of `service_type` on `port` under `instance_name` -- degrades,
/// never fails the caller's own server-start: a LAN this console can't multicast on (or a
/// binary not built with the `mdns` feature) still leaves the real MCP/A2A server itself
/// perfectly reachable by explicit host/port via `/mcp-call`/`/a2a-call`, exactly as before this
/// slice existed. Returns a real, honest sentence either way, appended to the server-start
/// message.
fn advertise_and_describe(service_type: &str, instance_name: &str, port: u16) -> String {
    // The returned handle is allowed to simply go out of scope here -- see `start_mcp_server`'s
    // own comment on why that's enough: the real background daemon it wraps keeps the service
    // published regardless of this particular handle's lifetime.
    match discovery::advertise(service_type, instance_name, port) {
        Ok(_advertisement) => format!(
            " Also advertising as \"{instance_name}\" on {service_type} for the rest of this \
             process's life."
        ),
        Err(e) => format!(" (Not advertised on the LAN: {e}.)"),
    }
}

/// `/mcp-discover [seconds]`/`/a2a-discover [seconds]` -- the real browse half: scans the real
/// LAN for `service_type` for [`DEFAULT_DISCOVERY_TIMEOUT`] (or `seconds`, if given) and lists
/// every real peer resolved. See [`discovery`]'s own doc comment on what this does and doesn't
/// prove about a listed peer.
fn discover_peers(service_type: &str, seconds_arg: &str) -> String {
    let timeout = if seconds_arg.is_empty() {
        DEFAULT_DISCOVERY_TIMEOUT
    } else {
        match seconds_arg.parse() {
            Ok(secs) => Duration::from_secs(secs),
            Err(_) => {
                return format!(
                    "\"{seconds_arg}\" isn't a real number of seconds -- try \"/mcp-discover\" \
                     or \"/mcp-discover 5\"."
                )
            }
        }
    };
    match discovery::discover(service_type, timeout) {
        Ok(peers) if peers.is_empty() => {
            format!("No real peers answered for {service_type} within {timeout:?}.")
        }
        Ok(peers) => {
            let mut lines = vec![format!(
                "Found {} real peer(s) for {service_type}:",
                peers.len()
            )];
            for peer in peers {
                lines.push(format!(
                    "  {} -- {} ({})",
                    peer.instance_name, peer.addr, peer.host
                ));
            }
            lines.join("\n")
        }
        Err(e) => format!("I couldn't scan the LAN for {service_type}: {e}"),
    }
}

/// `/mcp-server [port]` -- starts a real MCP server in a real background thread (default port
/// [`DEFAULT_MCP_PORT`]), wrapping this same live session; see [`mcp`]'s own doc comment for
/// exactly what it exposes. Returns immediately -- the server keeps running in the background for
/// the rest of this process's life (stop the process, or see `/standby` to keep it alive
/// intentionally while testing from elsewhere).
fn start_mcp_server(session: &Arc<Mutex<ConsoleSession>>, port_arg: &str) -> ControlOutcome {
    let port = if port_arg.is_empty() {
        DEFAULT_MCP_PORT
    } else {
        match port_arg.parse() {
            Ok(port) => port,
            Err(_) => {
                return ControlOutcome::Handled(vec![format!(
                    "\"{port_arg}\" isn't a real port number -- try \"/mcp-server\" or \
                     \"/mcp-server 8765\"."
                )])
            }
        }
    };
    match mcp::spawn_server(session.clone(), port) {
        Ok(server) => {
            let addr = server.addr();
            // Deliberately dropped, not stopped: `RunningServer` has no `Drop` side effect (its
            // real background thread owns its own state, not a borrow of this handle), so this
            // server keeps serving until the whole process ends -- see this function's own doc
            // comment.
            drop(server);
            let advertised =
                advertise_and_describe(discovery::MCP_SERVICE_TYPE, "hyperion-mcp", addr.port());
            ControlOutcome::Handled(vec![format!(
                "Real MCP server listening on http://{addr} -- JSON-RPC 2.0 (initialize, \
                 tools/list, tools/call). Still running when this command returns; use \
                 \"/standby\" to keep this process alive while you test it from elsewhere.\
                 {advertised}"
            )])
        }
        Err(e) => ControlOutcome::Handled(vec![format!("I couldn't start the MCP server: {e}")]),
    }
}

/// `/a2a-server [port]` -- as [`start_mcp_server`], for a real A2A server instead; see [`a2a`]'s
/// own doc comment for exactly what it exposes.
fn start_a2a_server(session: &Arc<Mutex<ConsoleSession>>, port_arg: &str) -> ControlOutcome {
    let port = if port_arg.is_empty() {
        DEFAULT_A2A_PORT
    } else {
        match port_arg.parse() {
            Ok(port) => port,
            Err(_) => {
                return ControlOutcome::Handled(vec![format!(
                    "\"{port_arg}\" isn't a real port number -- try \"/a2a-server\" or \
                     \"/a2a-server 8766\"."
                )])
            }
        }
    };
    match a2a::spawn_server(session.clone(), port) {
        Ok(server) => {
            let addr = server.addr();
            // See `start_mcp_server`'s own comment on why a plain drop is enough here.
            drop(server);
            let advertised =
                advertise_and_describe(discovery::A2A_SERVICE_TYPE, "hyperion-a2a", addr.port());
            ControlOutcome::Handled(vec![format!(
                "Real A2A server listening on http://{addr} -- Agent Card at \
                 /.well-known/agent-card.json, SendMessage at /. Still running when this command \
                 returns; use \"/standby\" to keep this process alive while you test it from \
                 elsewhere.{advertised}"
            )])
        }
        Err(e) => ControlOutcome::Handled(vec![format!("I couldn't start the A2A server: {e}")]),
    }
}

/// `/standby` -- blocks on a real, literal read of this process's own real stdin (never the
/// scenario file [`run_scenario_file`] might otherwise still be reading from) until the user
/// actually provides input, then ends the process. The one real reason this exists: keep a
/// process that just started a real background server (`/mcp-server`/`/a2a-server`) alive long
/// enough to actually test it from another terminal, entirely on the user's own schedule, rather
/// than the scenario file simply reaching its end and the whole process (server included) exiting
/// before anyone got the chance.
fn standby() -> ControlOutcome {
    println!("Standing by -- press Enter at this terminal when you're done testing, to stop.");
    let _ = io::stdout().flush();
    let mut line = String::new();
    let _ = io::stdin().lock().read_line(&mut line);
    ControlOutcome::Exit
}

/// `/mcp-call <host> <port> <tool> <json arguments>` -- the real outbound half: calls a real,
/// already-known MCP endpoint's `tools/call` (including another Hyperion instance's own
/// `/mcp-server`). Not discovery: the caller names the endpoint.
fn mcp_call(rest: &str) -> String {
    let mut parts = rest.splitn(4, ' ');
    let (Some(host), Some(port_str), Some(tool), Some(json_args)) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return "\"/mcp-call\" needs <host> <port> <tool> <json arguments>, e.g. \"/mcp-call \
                127.0.0.1 8765 hyperion.ask {\\\"prompt\\\":\\\"hello\\\"}\""
            .to_string();
    };
    let Ok(port) = port_str.parse::<u16>() else {
        return format!("\"{port_str}\" isn't a real port number.");
    };
    let arguments: serde_json::Value = match serde_json::from_str(json_args) {
        Ok(v) => v,
        Err(e) => return format!("that last argument isn't valid JSON: {e}"),
    };
    match mcp::call_tool(host, port, tool, arguments) {
        Ok(text) => text,
        Err(e) => format!("I couldn't call that MCP tool: {e}"),
    }
}

/// `/a2a-call <host> <port> <message text>` -- the real outbound half: sends a real message to a
/// real, already-known A2A endpoint's `SendMessage` method, checked against a real, persisted
/// peer identity (see [`a2a::send_message`]'s own doc comment).
fn a2a_call(rest: &str, data_dir: &str) -> String {
    let mut parts = rest.splitn(3, ' ');
    let (Some(host), Some(port_str), Some(text)) = (parts.next(), parts.next(), parts.next())
    else {
        return "\"/a2a-call\" needs <host> <port> <message text>, e.g. \"/a2a-call 127.0.0.1 \
                8766 hello there\""
            .to_string();
    };
    let Ok(port) = port_str.parse::<u16>() else {
        return format!("\"{port_str}\" isn't a real port number.");
    };
    let mut trust_store = match hyperion_console::peer_trust::PeerTrustStore::open_or_create(
        peer_trust_path(data_dir),
    ) {
        Ok(store) => store,
        Err(e) => return format!("I couldn't open the real peer trust store: {e}"),
    };
    match a2a::send_message(host, port, text, &mut trust_store) {
        Ok(reply) => reply,
        Err(e) => format!("I couldn't send that A2A message: {e}"),
    }
}

/// Feeds a real scenario file, one real utterance per line, through the exact same
/// [`ConsoleSession::handle_utterance_with_progress`] path [`run_interactive`] uses -- a scenario
/// file is a *record* of the same real turns a person could have typed, not a distinct code path.
/// Echoes each utterance before its response (`"> {utterance}"`) since nothing else would --
/// unlike a real terminal, a file's own lines were never typed anywhere visible -- except while
/// [`ConsoleSession::awaiting_secret_input`] is true, when the real pasted API key is redacted in
/// this echo exactly as [`hyperion_console::secret_input::RawEchoOff`] keeps it off a real
/// terminal. No banner, no trailing interactive prompt: a scenario file's output is meant to be a
/// reviewable transcript, not a chat session.
fn run_scenario_file(path: &str, session: &Arc<Mutex<ConsoleSession>>, data_dir: &str) {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(e) => {
            eprintln!("I couldn't read the scenario file {path:?}: {e}.");
            std::process::exit(1);
        }
    };

    for raw_line in contents.lines() {
        let trimmed = raw_line.trim();
        // Checked before deciding whether this line is "just" a comment/blank spacer -- an empty
        // line while awaiting a secret is itself a real, legitimate answer (cancel connecting),
        // the same rule `run_interactive` already applies to a real typed empty line.
        let awaiting_secret = session.lock().unwrap().awaiting_secret_input();
        if !awaiting_secret && (trimmed.is_empty() || trimmed.starts_with('#')) {
            continue;
        }

        let utterance = expand_env_vars(trimmed);
        if awaiting_secret {
            println!("{}", color::prompt("> [key redacted]"));
        } else {
            println!("{}", color::prompt(&format!("> {utterance}")));
        }

        if !awaiting_secret {
            match try_control_command(&utterance, session, data_dir) {
                ControlOutcome::Exit => return,
                ControlOutcome::Handled(lines) => {
                    for line in lines {
                        println!("{}", color::status_line(&line));
                    }
                    println!();
                    continue;
                }
                ControlOutcome::NotRecognized => {}
            }
        }

        let output_lines =
            session
                .lock()
                .unwrap()
                .handle_utterance_with_progress(&utterance, &mut |event| {
                    if let TaskProgress::Done(line) = event {
                        println!("{}", color::status_line(&line));
                    }
                });
        for output_line in output_lines {
            println!("{}", color::status_line(&output_line));
        }
        println!();
    }

    if session.lock().unwrap().awaiting_secret_input() {
        eprintln!(
            "Scenario file ended while still waiting for a pasted API key -- that \"connect\" \
             never completed."
        );
    }
}

/// Expands `$NAME` references (letters, digits, underscore) against this real process's own
/// environment -- the same interpolation a shell would already do for the
/// `printf '%s\n' "$OPENAI_API_KEY" ... | hyperion-console` pattern docs/999-usage-scenarios.md documents,
/// needed here because [`run_scenario_file`] reads its file's lines literally, with no shell in
/// between to do it. An unset reference is left untouched, not replaced with an empty string, so
/// a scenario author sees an honest failure downstream (e.g. "you haven't connected your openai
/// account yet") instead of a silently blank secret.
fn expand_env_vars(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        let mut name = String::new();
        while let Some(&next) = chars.peek() {
            if next.is_ascii_alphanumeric() || next == '_' {
                name.push(next);
                chars.next();
            } else {
                break;
            }
        }
        match std::env::var(&name) {
            Ok(value) if !name.is_empty() => out.push_str(&value),
            _ => {
                out.push('$');
                out.push_str(&name);
            }
        }
    }
    out
}

/// The real, live stdin/stdout chat loop -- unchanged from before scenario files existed, just
/// pulled into its own function so [`main`] can choose it or [`run_scenario_file`].
fn run_interactive(session: &Arc<Mutex<ConsoleSession>>, data_dir: &str) {
    // Only for a real interactive terminal -- a screen reader, a pipe, or a redirected/scripted
    // caller gets straight to the one line that actually matters, not decorative noise before it.
    if io::stdout().is_terminal() {
        println!();
        println!("{}", color::banner(BANNER));
    }
    println!();
    println!("You ask. I understand.");
    println!();

    let stdin = io::stdin();
    let mut input = stdin.lock();
    loop {
        print!("{}", color::prompt("> "));
        if io::stdout().flush().is_err() {
            break;
        }

        // A "connect my <provider> account" flow's follow-up API-key line must not be echoed to
        // the terminal or left sitting in scrollback -- checked before the real read, since
        // ECHO has to be off *during* it, not after `handle_utterance` already has the line.
        let awaiting_secret = session.lock().unwrap().awaiting_secret_input();
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

        if !awaiting_secret {
            match try_control_command(utterance, session, data_dir) {
                ControlOutcome::Exit => break,
                ControlOutcome::Handled(lines) => {
                    for line in lines {
                        println!("{}", color::status_line(&line));
                    }
                    println!();
                    continue;
                }
                ControlOutcome::NotRecognized => {}
            }
        }

        // A real `Spinner` animates while a tick of a decomposed multi-task plan is still
        // blocked on its own real (potentially slow) capability dispatch -- see
        // `ConsoleSession::run_decomposed_plan`'s own doc comment for why `Starting` fires
        // *before* that blocking call, not only `Done` after it. This closure is the one real
        // place in this crate allowed to touch stdout directly mid-turn.
        let interactive = io::stdout().is_terminal();
        let mut spinner: Option<Spinner> = None;
        let output_lines =
            session
                .lock()
                .unwrap()
                .handle_utterance_with_progress(utterance, &mut |event| match event {
                    TaskProgress::Starting(names) => {
                        if interactive && !names.is_empty() {
                            spinner = Some(Spinner::start(&names));
                        }
                    }
                    TaskProgress::Done(line) => {
                        if let Some(s) = spinner.take() {
                            s.stop();
                        }
                        println!("{}", color::status_line(&line));
                    }
                });
        // A plan that errors or breaks out of its own tick loop before a real `Done` event
        // fires would otherwise leave the spinner animating forever -- stop it here too.
        if let Some(s) = spinner.take() {
            s.stop();
        }

        for output_line in output_lines {
            println!("{}", color::status_line(&output_line));
        }
        println!();
    }
}
