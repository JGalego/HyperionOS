//! A minimal, real, dependency-free HTTP/1.1 server -- shared plumbing for `/mcp-server` and
//! `/a2a-server` (AUTONOMY_ROADMAP.md's Social pillar). Not a general-purpose web server: parses
//! just enough of a real HTTP/1.1 request (method, path, `Content-Length`, body) to dispatch to a
//! handler, and writes a real, minimal HTTP/1.1 response. Real sockets, real bytes over the wire --
//! an external client (`curl`, a real MCP/A2A client, another Hyperion instance) genuinely
//! connects to this, nothing simulated. Deliberately the same shape
//! `hyperion-console/tests/common/mod.rs`'s own fixture server already uses for *test* HTTP
//! servers -- duplicated here (not shared across that test-only module and this real one) since
//! this is real production code, not a test fixture.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// How often the real background accept loop checks whether [`RunningServer::stop`] was called --
/// bounds how quickly `stop` actually returns, not a request-handling latency (each accepted
/// connection is handled on its own thread, immediately).
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// A real, running HTTP server's handle. Dropping this does *not* stop the server -- it keeps
/// serving in the background for the rest of the process's life, matching every other
/// "runs until the process ends" real background thread this binary already spawns (e.g. the
/// interactive loop's own `Spinner`, active for its own turn's duration, or the process itself,
/// active for its own whole run). Call [`Self::stop`] to end it early -- this crate's own tests do.
pub struct RunningServer {
    addr: SocketAddr,
    // Genuinely real, tested API (this module's own `#[cfg(test)]` unit test exercises `stop`
    // directly) -- but `main.rs`'s own production call sites (`/mcp-server`/`/a2a-server`) never
    // call it, by design (see `Self`'s own doc comment on why a plain `drop` is enough there).
    // `cargo test -p hyperion-console --test <integration test>` builds a *separate*, plain
    // (non-`#[cfg(test)]`) copy of this bin to satisfy `CARGO_BIN_EXE_hyperion-console`, and
    // dead-code analysis is per build -- that copy alone would flag these as unused without this.
    #[allow(dead_code)]
    running: Arc<AtomicBool>,
    #[allow(dead_code)]
    handle: thread::JoinHandle<()>,
}

impl RunningServer {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Stops the real background accept loop and waits for it to actually exit.
    #[allow(dead_code)]
    pub fn stop(self) {
        self.running.store(false, Ordering::Relaxed);
        let _ = self.handle.join();
    }
}

/// A real request handler: `(method, path, body) -> (status_code, content_type, response_body)`.
pub trait Handler:
    Fn(&str, &str, &str) -> (u16, &'static str, String) + Send + Sync + 'static
{
}
impl<T: Fn(&str, &str, &str) -> (u16, &'static str, String) + Send + Sync + 'static> Handler for T {}

/// Spawns a real HTTP/1.1 server on `port` (`0` picks a real free ephemeral port -- read the one
/// actually bound via [`RunningServer::addr`]), calling `handler` for each real request received.
/// Runs in a real background thread until the process ends or [`RunningServer::stop`] is called --
/// a real non-blocking accept loop (bounded by [`POLL_INTERVAL`]) is what makes `stop` actually
/// responsive instead of blocking forever inside a plain `accept()`.
pub fn spawn(port: u16, handler: impl Handler) -> std::io::Result<RunningServer> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let running = Arc::new(AtomicBool::new(true));
    let running_thread = running.clone();
    let handler = Arc::new(handler);

    let handle = thread::spawn(move || {
        while running_thread.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let handler = handler.clone();
                    thread::spawn(move || handle_connection(stream, handler.as_ref()));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(POLL_INTERVAL);
                }
                Err(_) => break,
            }
        }
    });

    Ok(RunningServer {
        addr,
        running,
        handle,
    })
}

fn handle_connection(mut stream: TcpStream, handler: &impl Handler) {
    let Some((method, path, body)) = read_request(&mut stream) else {
        return;
    };
    let (status, content_type, response_body) = handler(&method, &path, &body);
    let status_text = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Unknown",
    };
    let response = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{response_body}",
        response_body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Reads one real raw HTTP/1.1 request off a real socket far enough to know its method,
/// query-stripped path, and (if present) body -- the same shape
/// `hyperion-console/tests/common/mod.rs`'s own fixture server parsing already uses.
fn read_request(stream: &mut TcpStream) -> Option<(String, String, String)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        let Some(header_end) = find_subslice(&buf, b"\r\n\r\n") else {
            continue;
        };

        let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
        let request_line = headers.lines().next().unwrap_or_default();
        let raw_path = request_line
            .split_whitespace()
            .nth(1)
            .unwrap_or_default()
            .to_string();
        let method = request_line
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string();
        let path = raw_path
            .split_once('?')
            .map(|(path, _query)| path.to_string())
            .unwrap_or(raw_path);

        let content_length: usize = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(|v| v.trim().to_string())
            })
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        let body_start = header_end + 4;
        while buf.len() < body_start + content_length {
            let n = stream.read(&mut chunk).ok()?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let body =
            String::from_utf8_lossy(&buf[body_start..buf.len().min(body_start + content_length)])
                .to_string();
        return Some((method, path, body));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpStream;

    /// Proves the real mechanics directly (bind, real request/response round trip, clean
    /// `stop`) -- `mcp`/`a2a`'s own tests prove the real protocols built on top of this.
    #[test]
    fn a_real_client_gets_a_real_response_and_stop_actually_ends_the_thread() {
        let server = spawn(0, |method, path, body| {
            (200, "text/plain", format!("{method} {path} {body}"))
        })
        .expect("bind a real ephemeral port");
        let addr = server.addr();

        let mut stream = TcpStream::connect(addr).expect("connect to the real server");
        stream
            .write_all(b"POST /hello HTTP/1.1\r\nContent-Length: 5\r\n\r\nhowdy")
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(
            response.contains("POST /hello howdy"),
            "expected the real handler's own response, got: {response:?}"
        );

        server.stop();
        assert!(
            TcpStream::connect(addr).is_err(),
            "a stopped server must no longer accept real connections"
        );
    }
}
