//! Shared real HTTP/1.1 fixture-server plumbing for this crate's cloud-backend tests
//! (`anthropic_backend.rs`, `gemini_backend.rs`) -- extracted once it was needed a third time
//! (`openai_compat_backend.rs` has its own, earlier copy of the same shape). A `tests/common/`
//! subdirectory, not a bare `tests/common.rs`, so cargo's test-binary auto-discovery doesn't
//! treat this as its own (empty) test target.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Reads one real raw HTTP/1.1 request off a real socket far enough to know its method,
/// query-stripped path, and (if present) body.
fn read_request(stream: &mut TcpStream) -> (String, String, String) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream
            .read(&mut chunk)
            .expect("read a real request off a real socket");
        if n == 0 {
            return (String::new(), String::new(), String::new());
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
            let n = stream
                .read(&mut chunk)
                .expect("read the rest of a real request body off a real socket");
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let body =
            String::from_utf8_lossy(&buf[body_start..buf.len().min(body_start + content_length)])
                .to_string();
        return (method, path, body);
    }
}

fn write_json_response(stream: &mut TcpStream, body: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .expect("write a real response to a real socket");
}

/// Spawns a real, minimal HTTP/1.1 fixture server on an ephemeral local port, handling exactly
/// `request_count` real requests before exiting. `handler(method, query-stripped path, body)`
/// returns the real JSON response body to send back for each one. Returns the real `base_url`
/// (`http://127.0.0.1:<port>`, no path) to connect a backend to.
pub fn spawn_fixture_server(
    request_count: usize,
    handler: impl Fn(&str, &str, &str) -> String + Send + 'static,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a real ephemeral local port");
    let addr = listener
        .local_addr()
        .expect("a real bound socket has a real local address");

    thread::spawn(move || {
        for _ in 0..request_count {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let (method, path, body) = read_request(&mut stream);
            let response_body = handler(&method, &path, &body);
            write_json_response(&mut stream, &response_body);
        }
    });

    format!("http://{addr}")
}
