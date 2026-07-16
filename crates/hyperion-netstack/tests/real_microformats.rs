//! docs/19-networking-stack.md's own previously-named "no real schema.org/JSON-LD/OpenGraph
//! microformat parser exists" gap, proven for real: `ReqwestFetchBackend` really parses a real
//! fetched page's own JSON-LD/OpenGraph markup and populates `FetchedPage::structured` with it --
//! not always `None`. `NetstackHub::web_research` preferring a real structured signal over the
//! model-based fallback is already proven in `extraction_and_resolution.rs` (via
//! `MockFetchBackend`); that same path can't be re-proven against a real local fixture server
//! here, since this crate's own SSRF containment (`canonical::is_private_or_local`) correctly
//! refuses a loopback target like `127.0.0.1` at the `NetstackHub` layer -- exactly why
//! `real_web_fetch.rs`'s own hub-level tests fetch real remote hosts (`example.com`) instead of a
//! local fixture. This file tests `ReqwestFetchBackend::fetch` directly, one layer below that
//! check, which is the layer this gap actually lives in.
//!
//! A hand-rolled local HTTP/1.1 fixture server, matching this crate's own `real_robots.rs` and
//! `real_web_fetch.rs` conventions -- no flaky remote dependency.
//!
//! `#[cfg(feature = "real-http")]`-gated like the backend itself -- invoke explicitly with
//! `cargo test -p hyperion-netstack --features real-http --test real_microformats`.

#![cfg(feature = "real-http")]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

use hyperion_netstack::{FetchBackend, ReqwestFetchBackend};

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn read_request_path(stream: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream
            .read(&mut chunk)
            .expect("read a real request off a real socket");
        if n == 0 {
            return String::new();
        }
        buf.extend_from_slice(&chunk[..n]);
        if find_subslice(&buf, b"\r\n\r\n").is_none() {
            continue;
        }
        let headers = String::from_utf8_lossy(&buf).to_string();
        return headers
            .lines()
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .nth(1)
            .unwrap_or_default()
            .to_string();
    }
}

/// Spawns a real, minimal HTTP/1.1 fixture server serving `page_body` at `/page` and a real,
/// always-allowing `robots.txt` at `/robots.txt` (so this test's own real requests aren't
/// incidentally disallowed and skipped by the real robots.txt check this crate's own
/// `ReqwestFetchBackend` performs before every fetch).
fn spawn_fixture_server(page_body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a real ephemeral local port");
    let addr = listener.local_addr().unwrap();

    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let path = read_request_path(&mut stream);
            if path.is_empty() {
                return;
            }
            let body = if path == "/robots.txt" { "" } else { page_body };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });

    format!("http://{addr}")
}

#[test]
fn a_real_json_ld_page_populates_structured_with_a_real_signal() {
    let base_url = spawn_fixture_server(
        r#"<html><head>
            <script type="application/ld+json">
                {"@type": "Person", "@id": "https://example.com/people/ada", "name": "Ada Lovelace"}
            </script>
        </head></html>"#,
    );

    let backend = ReqwestFetchBackend::new().unwrap();
    let page = backend.fetch(&format!("{base_url}/page")).unwrap();

    let structured = page
        .structured
        .expect("a real JSON-LD block must populate a real structured signal");
    assert_eq!(
        structured.identifier.as_deref(),
        Some("https://example.com/people/ada")
    );
    assert_eq!(structured.fields["name"], "Ada Lovelace");
}

#[test]
fn a_real_plain_page_leaves_structured_none() {
    let base_url =
        spawn_fixture_server("<html><head><title>Just a page</title></head><body></body></html>");

    let backend = ReqwestFetchBackend::new().unwrap();
    let page = backend.fetch(&format!("{base_url}/page")).unwrap();

    assert!(page.structured.is_none());
}

#[test]
fn a_real_open_graph_only_page_populates_structured_via_the_fallback_path() {
    let base_url = spawn_fixture_server(
        r#"<html><head>
            <meta property="og:title" content="A Real Product">
            <meta property="og:type" content="Product">
        </head></html>"#,
    );

    let backend = ReqwestFetchBackend::new().unwrap();
    let page = backend.fetch(&format!("{base_url}/page")).unwrap();

    let structured = page
        .structured
        .expect("real OpenGraph tags with no JSON-LD block must still populate a signal");
    assert_eq!(structured.fields["title"], "A Real Product");
}
