//! A minimal, real, dependency-free HTTP/1.1 server -- shared plumbing for `/mcp-server` and
//! `/a2a-server` (docs/998-roadmap.md's Social pillar). Not a general-purpose web server: parses
//! just enough of a real HTTP/1.1 request (method, path, `Content-Length`, body) to dispatch to a
//! handler, and writes a real, minimal HTTP/1.1 response. Real sockets, real bytes over the wire --
//! an external client (`curl`, a real MCP/A2A client, another Hyperion instance) genuinely
//! connects to this, nothing simulated. Deliberately the same shape
//! `hyperion-console/tests/common/mod.rs`'s own fixture server already uses for *test* HTTP
//! servers -- duplicated here (not shared across that test-only module and this real one) since
//! this is real production code, not a test fixture.
//!
//! **What guards it.** Everything served here is unauthenticated and drives a real agent turn, so
//! the only access control is that it binds loopback. [`reject_reason`] is what keeps that from
//! being defeated by a browser -- the one program on the user's machine that runs code chosen by
//! whoever they last visited. Read it before adding a handler, and again before widening the bind
//! address past `127.0.0.1`, which those checks currently assume.

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

/// How long one connection gets to deliver its request before it's dropped.
///
/// Without this, a client that opens a socket and then says nothing (or stops halfway through a
/// body it promised in `Content-Length`) holds its handler thread forever. One such connection per
/// thread is all it takes to exhaust the process, and nothing here ever reclaims them.
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a connection may take to close its own end after the response is written -- see
/// [`close_gracefully`]. Much shorter than [`REQUEST_READ_TIMEOUT`]: the exchange is already over,
/// and a peer that never closes is not owed the same patience as one mid-request.
const LINGER_TIMEOUT: Duration = Duration::from_secs(2);

/// Hard ceiling on a request's header block. Reached without a `\r\n\r\n` in sight, the connection
/// is refused rather than buffered indefinitely -- a headers-only stream is otherwise unbounded.
const MAX_HEADER_BYTES: usize = 64 * 1024;

/// Hard ceiling on a request body, regardless of what `Content-Length` claims. `Content-Length` is
/// attacker-controlled: honouring a declared multi-gigabyte body means allocating for it.
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

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
    // Both directions: a client that stalls mid-request and one that never drains the response
    // pin a thread the same way.
    let _ = stream.set_read_timeout(Some(REQUEST_READ_TIMEOUT));
    let _ = stream.set_write_timeout(Some(REQUEST_READ_TIMEOUT));

    let Some(request) = read_request(&mut stream) else {
        return;
    };

    let (status, content_type, response_body) = match reject_reason(&request) {
        Some(reason) => (
            403,
            "application/json",
            serde_json::json!({"error": reason}).to_string(),
        ),
        None => handler(&request.method, &request.path, &request.body),
    };
    let status_text = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        _ => "Unknown",
    };
    let response = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{response_body}",
        response_body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    close_gracefully(stream);
}

/// Ends a connection so the peer reliably receives what was just written to it.
///
/// `Connection: close` promises the socket ends here, and simply dropping the `TcpStream` leaves
/// that to `close(2)`. On BSD-derived stacks -- macOS, in this project's CI -- closing a socket
/// that still has anything unread in its receive queue sends an RST, and an RST **discards the
/// send buffer**. The response can be written, complete and correct, and never arrive: the client
/// gets `ConnectionReset` where the reply should have been. That is not a test artifact; it is a
/// server that loses its own answers, intermittently, on one platform.
///
/// The fix is the standard graceful close: half-close so the peer sees a clean FIN, then read
/// until it closes too. Draining leaves the receive queue empty, so the final `close(2)` has no
/// reason to reset, and the FIN tells a well-behaved client the response is complete.
///
/// Bounded by [`LINGER_TIMEOUT`] rather than the request timeout: a client that never closes its
/// end shouldn't hold this thread for the same generous window a client mid-request gets.
fn close_gracefully(mut stream: TcpStream) {
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let _ = stream.set_read_timeout(Some(LINGER_TIMEOUT));
    let mut sink = [0u8; 1024];
    while matches!(stream.read(&mut sink), Ok(n) if n > 0) {}
}

/// Why this request must not be served, or `None` to serve it.
///
/// These servers bind loopback and answer without authentication -- `/mcp-server`'s `tools/call`
/// and `/a2a-server`'s `SendMessage` each drive a real turn through the real Intent Engine, real
/// Agent dispatch, and real Knowledge Graph reads. "Only reachable from this machine" is doing all
/// the access control, and a browser is a program on this machine that runs code chosen by
/// whatever page the user happens to have open. Two checks close the two ways such a page reaches
/// a loopback port:
///
/// - **`Origin`**: any page can `fetch("http://127.0.0.1:8766/", ...)`. The request goes out
///   regardless; what stopped the page from *reading* the answer was the same-origin policy --
///   until this server volunteered `Access-Control-Allow-Origin: *` on every response, which
///   instructed the browser to hand it over. That header is gone, and a cross-site `Origin` is now
///   refused outright, so the request doesn't reach the agent at all rather than merely having its
///   answer withheld. This is what MCP's own transport security section asks of local servers.
///
/// - **`Host`**: DNS rebinding defeats the first check by making the page's own origin loopback.
///   An attacker points `evil.example` at `127.0.0.1`, and the browser then treats
///   `http://evil.example:8766/` as same-origin with the target -- no `Origin` header sent, and
///   any response readable. The one thing that still gives it away is `Host: evil.example`, since
///   a genuine local client addresses this server by a loopback name.
///
/// Non-browser clients (`curl`, a real MCP/A2A client, another Hyperion instance over an SSH
/// tunnel) send no `Origin` and a loopback `Host`, and are unaffected.
fn reject_reason(request: &Request) -> Option<&'static str> {
    if let Some(origin) = request.header("origin") {
        if !is_loopback_origin(&origin) {
            return Some(
                "this endpoint does not serve cross-site browser requests -- see \
                 hyperion-console's http_server for why",
            );
        }
    }
    match request.header("host") {
        Some(host) if is_loopback_authority(&host) => None,
        // HTTP/1.1 requires `Host`; its absence is either a malformed client or an attempt to
        // dodge the check below, and neither is worth serving an unauthenticated agent turn to.
        _ => Some("this endpoint only answers requests addressed to it as a local service"),
    }
}

/// `true` if `origin` is `http(s)://<loopback authority>` -- i.e. a page this server itself served
/// (the mesh dashboard is exactly that, and calls back to `/mesh/graph`).
fn is_loopback_origin(origin: &str) -> bool {
    let authority = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"));
    authority.is_some_and(is_loopback_authority)
}

/// `true` if `authority` is a loopback host, with or without a port: `127.0.0.1`, `localhost`, or
/// `[::1]`. Any other name -- including one that currently *resolves* to loopback, which is
/// precisely the DNS-rebinding case -- is not.
fn is_loopback_authority(authority: &str) -> bool {
    let host = match authority.strip_prefix('[') {
        // IPv6 literal: `[::1]` / `[::1]:8766`.
        Some(rest) => match rest.split_once(']') {
            Some((inner, _port)) => inner,
            None => return false,
        },
        None => authority.split(':').next().unwrap_or_default(),
    };
    matches!(
        host.trim().to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "::1"
    )
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// One parsed HTTP/1.1 request: enough of it to dispatch on, plus the raw header block the
/// loopback checks in [`reject_reason`] need.
struct Request {
    method: String,
    path: String,
    headers: String,
    body: String,
}

impl Request {
    /// The value of `name` (matched case-insensitively, as HTTP header names are), or `None`.
    fn header(&self, name: &str) -> Option<String> {
        self.headers.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_string())
        })
    }
}

/// Reads one real raw HTTP/1.1 request off a real socket far enough to know its method,
/// query-stripped path, headers, and (if present) body -- the same shape
/// `hyperion-console/tests/common/mod.rs`'s own fixture server parsing already uses.
///
/// Every read is bounded, in three separate ways, because none of them are the same attack: the
/// socket's own [`REQUEST_READ_TIMEOUT`] caps how long a stalled client holds this thread,
/// [`MAX_HEADER_BYTES`] caps a client that streams headers forever without ever sending
/// `\r\n\r\n`, and [`MAX_BODY_BYTES`] caps one that declares a body far larger than it intends to
/// send (or than this process can hold). Returns `None` on any of them -- the connection is simply
/// dropped, since a client behaving this way has no legitimate response to be given.
fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(end) = find_subslice(&buf, b"\r\n\r\n") {
            break end;
        }
        if buf.len() > MAX_HEADER_BYTES {
            return None;
        }
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };

    let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let request_line = headers.lines().next().unwrap_or_default();
    let method = request_line
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
    let raw_path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_string();
    let path = raw_path
        .split_once('?')
        .map(|(path, _query)| path.to_string())
        .unwrap_or(raw_path);

    let declared_length: usize = headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().to_string())
        })
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if declared_length > MAX_BODY_BYTES {
        return None;
    }

    let body_start = header_end + 4;
    while buf.len() < body_start + declared_length {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body =
        String::from_utf8_lossy(&buf[body_start..buf.len().min(body_start + declared_length)])
            .to_string();

    Some(Request {
        method,
        path,
        headers,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpStream;

    /// A real, echoing server on a real ephemeral port, plus the raw request/response round trip
    /// the tests below drive it with.
    fn echo_server() -> RunningServer {
        spawn(0, |method, path, body| {
            (200, "text/plain", format!("{method} {path} {body}"))
        })
        .expect("bind a real ephemeral port")
    }

    /// Reads the whole response, treating a reset that arrives *after* the response bytes as the
    /// end of the exchange rather than a failure. The server half-closes deliberately, but a peer
    /// is still entitled to reset a connection it considers finished, and on macOS it sometimes
    /// does -- a test that insisted on a textbook FIN would be asserting on the platform's TCP
    /// stack, not on this server.
    fn round_trip(server: &RunningServer, request: &str) -> String {
        let mut stream = TcpStream::connect(server.addr()).expect("connect to the real server");
        stream.write_all(request.as_bytes()).unwrap();

        let mut response = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => response.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => break,
                Err(e) => panic!("reading the real response failed: {e}"),
            }
        }
        String::from_utf8_lossy(&response).into_owned()
    }

    /// Proves the real mechanics directly (bind, real request/response round trip, clean
    /// `stop`) -- `mcp`/`a2a`'s own tests prove the real protocols built on top of this.
    #[test]
    fn a_real_client_gets_a_real_response_and_stop_actually_ends_the_thread() {
        let server = echo_server();
        let addr = server.addr();

        let response = round_trip(
            &server,
            "POST /hello HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 5\r\n\r\nhowdy",
        );
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

    #[test]
    fn a_cross_site_browser_request_never_reaches_the_handler() {
        let server = echo_server();
        let response = round_trip(
            &server,
            "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: https://evil.example\r\n\
             Content-Length: 5\r\n\r\nhowdy",
        );
        assert!(
            response.starts_with("HTTP/1.1 403 "),
            "a page on another origin must not be able to drive a real agent turn, got: \
             {response:?}"
        );
        assert!(
            !response.contains("POST / howdy"),
            "the handler must not have run at all, got: {response:?}"
        );
        server.stop();
    }

    #[test]
    fn no_response_ever_carries_a_wildcard_cors_header() {
        let server = echo_server();
        let response = round_trip(&server, "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        assert!(
            !response
                .to_ascii_lowercase()
                .contains("access-control-allow-origin"),
            "this server must not instruct browsers to hand its responses to other origins, got: \
             {response:?}"
        );
        server.stop();
    }

    #[test]
    fn a_page_this_server_itself_served_may_still_call_back() {
        // The mesh dashboard is served from `/` and fetches `/mesh/graph`; a same-origin call
        // must keep working, or the Origin check would have broken the one real browser client.
        let server = echo_server();
        let origin = format!("http://{}", server.addr());
        let response = round_trip(
            &server,
            &format!(
                "GET /mesh/graph HTTP/1.1\r\nHost: {}\r\nOrigin: {origin}\r\n\r\n",
                server.addr()
            ),
        );
        assert!(
            response.contains("GET /mesh/graph"),
            "a same-origin call from this server's own page must be served, got: {response:?}"
        );
        server.stop();
    }

    #[test]
    fn a_rebound_dns_name_is_refused_even_though_it_resolves_to_loopback() {
        // What DNS rebinding produces: the browser believes it's talking to `evil.example` (so it
        // sends no cross-site `Origin`), but the name resolves here. `Host` is what still tells
        // the truth.
        let server = echo_server();
        let response = round_trip(&server, "POST / HTTP/1.1\r\nHost: evil.example\r\n\r\n");
        assert!(
            response.starts_with("HTTP/1.1 403 "),
            "a request addressed to a non-loopback name must be refused, got: {response:?}"
        );
        server.stop();
    }

    #[test]
    fn a_body_larger_than_the_cap_is_dropped_rather_than_allocated_for() {
        let server = echo_server();
        let mut stream = TcpStream::connect(server.addr()).expect("connect to the real server");
        // A declared 4 GiB body, none of which is actually sent.
        stream
            .write_all(b"POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 4294967296\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        assert!(
            response.is_empty(),
            "an over-cap Content-Length must drop the connection, not be honoured, got: \
             {response:?}"
        );
        server.stop();
    }

    #[test]
    fn loopback_authorities_are_recognized_with_and_without_ports() {
        for authority in [
            "127.0.0.1",
            "127.0.0.1:8766",
            "localhost",
            "LOCALHOST:8765",
            "[::1]",
            "[::1]:8766",
        ] {
            assert!(is_loopback_authority(authority), "{authority} is loopback");
        }
        for authority in [
            "evil.example",
            "evil.example:8766",
            "127.0.0.1.evil.example",
            "[::1",
            "",
        ] {
            assert!(
                !is_loopback_authority(authority),
                "{authority} is not loopback"
            );
        }
    }
}
