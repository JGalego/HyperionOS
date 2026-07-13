//! Proves `AnthropicBackend` for real, over a real local TCP socket -- same reasoning as
//! `openai_compat_backend.rs`: no real Anthropic account exists in this sandbox, so a hand-rolled
//! local fixture server proves the real HTTP/JSON wiring instead. Since this never leaves
//! loopback, it needs no separate exclusion from this feature's own default test run.

#![cfg(feature = "anthropic")]

mod common;

use hyperion_ai_runtime::{AnthropicBackend, InferenceBackend, InferenceRequest};

#[test]
fn connects_to_a_real_local_server_and_proves_a_real_request_response_round_trip() {
    let base_url = common::spawn_fixture_server(2, |method, path, body| match (method, path) {
        ("GET", "/models") => r#"{"data":[{"id":"test-model","type":"model"}]}"#.to_string(),
        ("POST", "/messages") => {
            let request: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
            let prompt = request["messages"][0]["content"]
                .as_str()
                .unwrap_or_default();
            format!(
                r#"{{"content":[{{"type":"text","text":"real anthropic fixture echo: {prompt}"}}]}}"#
            )
        }
        _ => r#"{"error":"unexpected request"}"#.to_string(),
    });

    let backend = AnthropicBackend::connect_at(base_url, "test-key", "test-model")
        .expect("connect to the real fixture server over a real local socket");

    let request = InferenceRequest {
        prompt: "what is the real meaning of this test".to_string(),
    };
    let text = backend.generate(1, &request);

    assert_eq!(
        text, "real anthropic fixture echo: what is the real meaning of this test",
        "expected the real fixture server's own real response to come back through generate(), \
         got: {text:?}"
    );
}

#[test]
fn an_unreachable_server_is_a_real_honest_connect_failure_not_a_panic() {
    let result = AnthropicBackend::connect_at("http://127.0.0.1:1", "test-key", "test-model");

    assert!(
        result.is_err(),
        "connecting to a real closed port must fail, not silently succeed"
    );
}
