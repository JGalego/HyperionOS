//! docs/19-networking-stack.md's own previously-named "`robots.txt` fetching/parsing" gap,
//! proven for real: `ReqwestFetchBackend` really fetches `robots.txt` over a real local socket
//! and really honors it -- a disallowed path is never even requested, and an allowed path (or a
//! host with no `robots.txt` at all) is fetched normally.
//!
//! A hand-rolled local HTTP/1.1 fixture server, not a real remote host -- the same move this
//! crate's own `real_web_fetch.rs` already documents making once, and this crate's cloud-backend
//! sibling `hyperion-ai-runtime` makes for its own real backends: no flaky remote dependency, and
//! (unlike a remote host) this server can assert on *which* real requests it did or didn't
//! receive.
//!
//! `#[cfg(feature = "real-http")]`-gated like the backend itself -- invoke explicitly with
//! `cargo test -p hyperion-netstack --features real-http --test real_robots`.

#![cfg(feature = "real-http")]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use hyperion_netstack::{FetchBackend, ReqwestFetchBackend};

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Reads one real raw HTTP/1.1 request off a real socket far enough to know its own real path --
/// this fixture never receives a body, so unlike `hyperion-ai-runtime`'s own `common::
/// spawn_fixture_server`, there is no `Content-Length` body to keep reading past the headers.
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
        let request_line = headers.lines().next().unwrap_or_default();
        return request_line
            .split_whitespace()
            .nth(1)
            .unwrap_or_default()
            .to_string();
    }
}

/// Spawns a real, minimal HTTP/1.1 fixture server on an ephemeral local port, serving whatever
/// `(status_line, body)` `routes` declares for each exact real path requested (a 404 for
/// anything else), and records every real path it actually received into `received` -- so a test
/// can assert a disallowed path was genuinely never fetched, not merely that the backend's
/// return value looked right.
fn spawn_fixture_server(
    routes: HashMap<&'static str, (&'static str, &'static str)>,
    received: Arc<Mutex<Vec<String>>>,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a real ephemeral local port");
    let addr = listener
        .local_addr()
        .expect("a real bound socket has a real local address");

    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let path = read_request_path(&mut stream);
            if path.is_empty() {
                return;
            }
            received.lock().unwrap().push(path.clone());
            let (status_line, body) = routes
                .get(path.as_str())
                .copied()
                .unwrap_or(("HTTP/1.1 404 Not Found", ""));
            let response = format!(
                "{status_line}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });

    format!("http://{addr}")
}

#[test]
fn a_disallowed_path_is_never_fetched_and_is_reported_as_robots_disallowed() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let mut routes = HashMap::new();
    routes.insert(
        "/robots.txt",
        ("HTTP/1.1 200 OK", "User-agent: *\nDisallow: /private/\n"),
    );
    let base_url = spawn_fixture_server(routes, received.clone());

    let backend = ReqwestFetchBackend::new().unwrap();
    let page = backend
        .fetch(&format!("{base_url}/private/secret"))
        .expect("a robots-disallowed fetch returns Ok with the flag set, not an Err");

    assert!(page.robots_disallowed);
    assert!(
        page.text.is_empty(),
        "a disallowed path's own content must never be returned"
    );

    // Give the fixture server a moment to record the request it (if any) received before this
    // assertion runs -- `fetch` itself already blocked on the real `robots.txt` round trip.
    let received = received.lock().unwrap();
    assert_eq!(
        received.as_slice(),
        &["/robots.txt".to_string()],
        "the disallowed page path itself must never have been requested at all"
    );
}

#[test]
fn an_allowed_path_is_fetched_normally_after_a_real_robots_txt_check() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let mut routes = HashMap::new();
    routes.insert(
        "/robots.txt",
        ("HTTP/1.1 200 OK", "User-agent: *\nDisallow: /private/\n"),
    );
    routes.insert("/public/page", ("HTTP/1.1 200 OK", "hello, world"));
    let base_url = spawn_fixture_server(routes, received.clone());

    let backend = ReqwestFetchBackend::new().unwrap();
    let page = backend
        .fetch(&format!("{base_url}/public/page"))
        .expect("an allowed path must fetch normally");

    assert!(!page.robots_disallowed);
    assert_eq!(page.text, "hello, world");
    assert_eq!(
        received.lock().unwrap().as_slice(),
        &["/robots.txt".to_string(), "/public/page".to_string()],
    );
}

#[test]
fn a_missing_robots_txt_allows_everything() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let mut routes = HashMap::new();
    routes.insert("/page", ("HTTP/1.1 200 OK", "no robots.txt here"));
    let base_url = spawn_fixture_server(routes, received.clone());

    let backend = ReqwestFetchBackend::new().unwrap();
    let page = backend
        .fetch(&format!("{base_url}/page"))
        .expect("a missing robots.txt (404) must allow everything, not fail closed");

    assert!(!page.robots_disallowed);
    assert_eq!(page.text, "no robots.txt here");
}

#[test]
fn robots_txt_is_fetched_at_most_once_per_host_across_multiple_page_fetches() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let mut routes = HashMap::new();
    routes.insert("/robots.txt", ("HTTP/1.1 200 OK", "User-agent: *\n"));
    routes.insert("/page-one", ("HTTP/1.1 200 OK", "one"));
    routes.insert("/page-two", ("HTTP/1.1 200 OK", "two"));
    let base_url = spawn_fixture_server(routes, received.clone());

    let backend = ReqwestFetchBackend::new().unwrap();
    backend.fetch(&format!("{base_url}/page-one")).unwrap();
    backend.fetch(&format!("{base_url}/page-two")).unwrap();

    let robots_txt_requests = received
        .lock()
        .unwrap()
        .iter()
        .filter(|p| p.as_str() == "/robots.txt")
        .count();
    assert_eq!(
        robots_txt_requests, 1,
        "a second real page fetch against the same host must reuse the cached robots.txt, not \
         re-fetch it"
    );
}
