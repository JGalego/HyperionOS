//! Proves `OpenAiCompatBackend` for real, over a real local TCP socket -- without needing a real
//! Ollama/vLLM/LiteLLM install in CI/sandbox. There's no stable, always-available public server
//! speaking this protocol to test against (unlike `hyperion-netstack`'s own `real_web_fetch.rs`,
//! which can hit real `example.com`), so this hand-rolls a minimal HTTP/1.1 fixture server on an
//! ephemeral local port instead -- the same move `real_web_fetch.rs`'s own doc comment records
//! this workspace already made once, for the same reason (a flaky remote test dependency,
//! replaced with a fully local, deterministic one). Since this never leaves loopback, it needs no
//! separate exclusion from this feature's own default test run.

#![cfg(feature = "openai-compat")]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

use hyperion_ai_runtime::{
    CancellationToken, InferenceBackend, InferenceRequest, OpenAiCompatBackend,
};

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Reads one real raw HTTP/1.1 request off a real socket far enough to know its method, path,
/// and (if present) body -- just enough of the protocol for this fixture's own two real request
/// shapes, not a general-purpose parser.
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
        let mut lines = headers.lines();
        let request_line = lines.next().unwrap_or_default();
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();

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
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .expect("write a real response to a real socket");
}

/// Spawns a real, minimal HTTP/1.1 fixture server on an ephemeral local port, handling exactly
/// two real requests -- `OpenAiCompatBackend::connect`'s own `GET /v1/models` health check, then
/// one real `POST /v1/chat/completions` -- before exiting. Returns the real `base_url` to connect
/// a backend to.
fn spawn_fixture_server(model: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a real ephemeral local port");
    let addr = listener
        .local_addr()
        .expect("a real bound socket has a real local address");

    thread::spawn(move || {
        for _ in 0..2 {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let (method, path, body) = read_request(&mut stream);
            match (method.as_str(), path.as_str()) {
                ("GET", "/v1/models") => write_json_response(
                    &mut stream,
                    &format!(r#"{{"object":"list","data":[{{"id":"{model}","object":"model"}}]}}"#),
                ),
                ("POST", "/v1/chat/completions") => {
                    let request: serde_json::Value = serde_json::from_str(&body)
                        .expect("a real, well-formed JSON chat-completion request body");
                    let prompt = request["messages"][0]["content"]
                        .as_str()
                        .unwrap_or_default();
                    write_json_response(
                        &mut stream,
                        &format!(
                            r#"{{"choices":[{{"message":{{"content":"real fixture echo: {prompt}"}}}}]}}"#
                        ),
                    );
                }
                _ => write_json_response(&mut stream, r#"{"error":"unexpected request"}"#),
            }
        }
    });

    format!("http://{addr}/v1")
}

#[test]
fn connects_to_a_real_local_server_and_proves_a_real_request_response_round_trip() {
    let base_url = spawn_fixture_server("test-model");

    let backend = OpenAiCompatBackend::connect(base_url, "test-model", None)
        .expect("connect to the real fixture server over a real local socket");

    let request = InferenceRequest {
        prompt: "what is the real meaning of this test".to_string(),
    };
    let text = backend.generate(1, &request, &CancellationToken::never_cancelled());

    assert_eq!(
        text, "real fixture echo: what is the real meaning of this test",
        "expected the real fixture server's own real response to come back through generate(), \
         not a fabricated or unrelated string, got: {text:?}"
    );
}

#[test]
fn an_unreachable_server_is_a_real_honest_connect_failure_not_a_panic() {
    // A real closed port on loopback -- nothing is listening here, so this is a real, immediate
    // connection failure, not a timeout this test would have to wait out.
    let result = OpenAiCompatBackend::connect("http://127.0.0.1:1", "test-model", None);

    assert!(
        result.is_err(),
        "connecting to a real closed port must fail, not silently succeed"
    );
}
