//! A minimal, real, dependency-free HTTP/1.1 client -- the outbound half of `/mcp-call`/
//! `/a2a-call` (AUTONOMY_ROADMAP.md's Social pillar: Hyperion calling *out* to a real, already-
//! known MCP/A2A endpoint, including another Hyperion instance's own `/mcp-server`/`/a2a-server`).
//! Deliberately hand-rolled rather than pulling in `reqwest` (already real elsewhere in this
//! workspace, e.g. `hyperion-netstack`): this client only ever needs one shape (`POST`/`GET` a
//! small JSON body, read a small JSON response, no redirects/cookies/streaming/TLS), and
//! `hyperion-console::http_server`'s own server side is equally hand-rolled -- one dependency-free
//! HTTP idiom for both directions, not a heavyweight client paired with a hand-rolled server.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Bounded so a real, unreachable, or hung remote endpoint fails fast and honestly rather than
/// blocking a console turn indefinitely -- the same "network calls need a real, bounded timeout"
/// principle `hyperion-ai-runtime::openai_compat_backend::CONNECT_TIMEOUT` already established,
/// tuned to a local-network round trip rather than that one's cloud-API budget.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// A real HTTP `GET` to `host:port` + `path`, returning the real response body (status ignored --
/// callers that care, like Agent Card fetch, check the body shape itself).
pub fn get(host: &str, port: u16, path: &str) -> Result<String, String> {
    request(host, port, "GET", path, None)
}

/// A real HTTP `POST` of `body` (sent as `application/json`) to `host:port` + `path`, returning
/// the real response body.
pub fn post(host: &str, port: u16, path: &str, body: &str) -> Result<String, String> {
    request(host, port, "POST", path, Some(body))
}

fn request(
    host: &str,
    port: u16,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<String, String> {
    let addr = format!("{host}:{port}");
    let mut stream =
        TcpStream::connect(&addr).map_err(|e| format!("couldn't reach {addr}: {e}"))?;
    stream
        .set_read_timeout(Some(READ_TIMEOUT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(CONNECT_TIMEOUT))
        .map_err(|e| e.to_string())?;

    let request = match body {
        Some(body) => format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
        None => format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"),
    };
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("couldn't send the real request: {e}"))?;

    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| format!("couldn't read the real response: {e}"))?;
    let raw = String::from_utf8_lossy(&raw);
    let Some((_headers, body)) = raw.split_once("\r\n\r\n") else {
        return Err("the response had no real HTTP header/body split".to_string());
    };
    Ok(body.to_string())
}
