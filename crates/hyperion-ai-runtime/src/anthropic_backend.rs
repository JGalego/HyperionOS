//! A real, feature-gated [`InferenceBackend`] speaking Anthropic's own real Messages API --
//! docs/998-roadmap.md's "Phase 2: cloud providers." Unlike [`crate::openai_compat_backend`]
//! (which covers Ollama/vLLM/LiteLLM/OpenAI itself via one OpenAI-compatible shape), Anthropic's
//! wire protocol is genuinely different -- its own `x-api-key`/`anthropic-version` headers, its
//! own request/response JSON shape -- so it needs its own dedicated backend.
//!
//! Gated behind the `anthropic` Cargo feature, the same "off by default, real behind an opt-in
//! feature" convention every other real backend in this crate already uses.
//!
//! **Named gap, not silently assumed:** this sandbox has no real Anthropic API key to verify
//! against live -- this module's tests prove its own real HTTP/JSON wiring against a hand-rolled
//! local fixture server (matching this crate's own `openai_compat_backend` test convention), not
//! a real account. Real end-to-end verification against a real key is a real follow-up step.

use std::time::Duration;

use serde_json::Value;

use crate::runtime::{CancellationToken, InferenceBackend};
use crate::types::InferenceRequest;

const BASE_URL: &str = "https://api.anthropic.com/v1";
/// Anthropic's own required request header identifying which dated API version this backend was
/// written against -- not optional, unlike most REST APIs' version headers.
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Anthropic's Messages API requires `max_tokens` explicitly (no server-side default, unlike
/// OpenAI's optional field) -- generous enough for a real, complete answer without being
/// unbounded.
const MAX_TOKENS: u32 = 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const GENERATE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, thiserror::Error)]
pub enum AnthropicError {
    #[error("couldn't build a real HTTP client: {0}")]
    ClientInit(#[source] reqwest::Error),
    #[error("couldn't reach the real Anthropic API ({0})")]
    Unreachable(#[source] reqwest::Error),
    #[error("the real Anthropic API responded with an unexpected status: {0}")]
    UnexpectedStatus(reqwest::StatusCode),
    #[error("couldn't parse the real Anthropic API's response: {0}")]
    ResponseParse(String),
}

/// A real backend for Anthropic's own Messages API. [`Self::generate`] is the entire
/// [`InferenceBackend`] contract: a real prompt in, a real completion out, via a real HTTP
/// round trip against `api.anthropic.com`.
pub struct AnthropicBackend {
    client: reqwest::blocking::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl AnthropicBackend {
    /// Connects to the real Anthropic API and proves reachability via a real `GET
    /// {base_url}/models` call, eagerly, at construction time -- the same "prove it now, not on
    /// the first real generate() call" reasoning
    /// [`crate::openai_compat_backend::OpenAiCompatBackend::connect`]'s own doc comment already
    /// gives (this trait's `generate()` can't return a `Result` to surface a deferred failure
    /// through).
    pub fn connect(
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self, AnthropicError> {
        Self::connect_at(BASE_URL, api_key, model)
    }

    /// As [`Self::connect`], against a caller-chosen `base_url` rather than the real Anthropic
    /// API -- a real API client feature (a regional deployment, a corporate proxy in front of
    /// Anthropic) that this crate's own tests also use to prove real HTTP/JSON wiring against a
    /// local fixture server, without needing a real account.
    pub fn connect_at(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self, AnthropicError> {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        let api_key = api_key.into();
        let model = model.into();

        let client = reqwest::blocking::Client::builder()
            .build()
            .map_err(AnthropicError::ClientInit)?;

        let response = client
            .get(format!("{base_url}/models"))
            .header("x-api-key", &api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .timeout(CONNECT_TIMEOUT)
            .send()
            .map_err(AnthropicError::Unreachable)?;
        if !response.status().is_success() {
            return Err(AnthropicError::UnexpectedStatus(response.status()));
        }
        let body: Value = response
            .json()
            .map_err(|e| AnthropicError::ResponseParse(e.to_string()))?;

        let model_is_known = body
            .get("data")
            .and_then(Value::as_array)
            .map(|models| {
                models
                    .iter()
                    .filter_map(|entry| entry.get("id").and_then(Value::as_str))
                    .any(|id| id == model)
            })
            .unwrap_or(false);
        if !model_is_known {
            eprintln!(
                "warning: {base_url} didn't list {model:?} among its own real models -- \
                 continuing anyway; a genuinely wrong name will surface on the first real \
                 request instead"
            );
        }

        Ok(AnthropicBackend {
            client,
            base_url,
            api_key,
            model,
        })
    }
}

impl InferenceBackend for AnthropicBackend {
    /// A real HTTP round trip against Anthropic's real Messages API, parsed loosely via
    /// [`serde_json::Value`] (fishing for `content[0].text`) rather than a strict typed
    /// `Deserialize`. Every failure is embedded as `"[anthropic backend error: ...]"` text,
    /// matching every other real backend's convention in this crate -- this trait's contract
    /// returns a plain `String`, never a `Result`. `cancel` is unused: one blocking HTTP call, no
    /// real per-chunk boundary -- see `hyperion_ai_runtime::CancellationToken`'s own doc comment.
    fn generate(
        &self,
        _model_id: u64,
        request: &InferenceRequest,
        _cancel: &CancellationToken,
    ) -> String {
        let payload = serde_json::json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "messages": [{ "role": "user", "content": request.prompt }],
        });

        let response = match self
            .client
            .post(format!("{}/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .timeout(GENERATE_TIMEOUT)
            .json(&payload)
            .send()
        {
            Ok(response) => response,
            Err(e) => {
                return format!(
                    "[anthropic backend error: couldn't reach the real Anthropic API: {e}]"
                )
            }
        };
        if !response.status().is_success() {
            return format!(
                "[anthropic backend error: the real Anthropic API responded with an unexpected \
                 status: {}]",
                response.status()
            );
        }
        let body: Value = match response.json() {
            Ok(body) => body,
            Err(e) => {
                return format!(
                    "[anthropic backend error: couldn't parse the real Anthropic API's \
                     response: {e}]"
                )
            }
        };

        body.get("content")
            .and_then(Value::as_array)
            .and_then(|content| content.first())
            .and_then(|block| block.get("text"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                "[anthropic backend error: the real Anthropic API's response had no \
                 content[0].text]"
                    .to_string()
            })
    }
}
