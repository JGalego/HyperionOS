//! A real, feature-gated [`InferenceBackend`] speaking Google's real Gemini
//! (`generativelanguage.googleapis.com`) API -- docs/998-roadmap.md's "Phase 2: cloud
//! providers." Like [`crate::anthropic_backend`], Gemini's wire protocol is genuinely different
//! from the OpenAI-compatible shape [`crate::openai_compat_backend`] already covers (a
//! `?key=...` query-string API key, a `contents`/`parts` request shape, a
//! `candidates[0].content.parts[0].text` response shape), so it needs its own dedicated backend.
//!
//! Gated behind the `gemini` Cargo feature, the same "off by default, real behind an opt-in
//! feature" convention every other real backend in this crate already uses.
//!
//! **Named gap, not silently assumed:** this sandbox has no real Gemini API key to verify
//! against live -- this module's tests prove its own real HTTP/JSON wiring against a hand-rolled
//! local fixture server (matching this crate's other real-backend test conventions), not a real
//! account. Real end-to-end verification against a real key is a real follow-up step.

use std::time::Duration;

use serde_json::Value;

use crate::runtime::{CancellationToken, InferenceBackend};
use crate::types::InferenceRequest;

const BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const GENERATE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, thiserror::Error)]
pub enum GeminiError {
    #[error("couldn't build a real HTTP client: {0}")]
    ClientInit(#[source] reqwest::Error),
    #[error("couldn't reach the real Gemini API ({0})")]
    Unreachable(#[source] reqwest::Error),
    #[error("the real Gemini API responded with an unexpected status: {0}")]
    UnexpectedStatus(reqwest::StatusCode),
    #[error("couldn't parse the real Gemini API's response: {0}")]
    ResponseParse(String),
}

/// A real backend for Google's own Gemini API. [`Self::generate`] is the entire
/// [`InferenceBackend`] contract: a real prompt in, a real completion out, via a real HTTP
/// round trip against `generativelanguage.googleapis.com`.
pub struct GeminiBackend {
    client: reqwest::blocking::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl GeminiBackend {
    /// Connects to the real Gemini API and proves reachability via a real `GET
    /// {base_url}/models?key=...` call, eagerly, at construction time -- same reasoning as
    /// [`crate::openai_compat_backend::OpenAiCompatBackend::connect`] and
    /// [`crate::anthropic_backend::AnthropicBackend::connect`]. Gemini's own model list entries
    /// are named `"models/{id}"`, not the bare `{id}` a caller passes here, so the check strips
    /// that prefix before comparing.
    pub fn connect(
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self, GeminiError> {
        Self::connect_at(BASE_URL, api_key, model)
    }

    /// As [`Self::connect`], against a caller-chosen `base_url` rather than the real Gemini API
    /// -- a real API client feature this crate's own tests also use to prove real HTTP/JSON
    /// wiring against a local fixture server, without needing a real account.
    pub fn connect_at(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self, GeminiError> {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        let api_key = api_key.into();
        let model = model.into();

        let client = reqwest::blocking::Client::builder()
            .build()
            .map_err(GeminiError::ClientInit)?;

        let response = client
            .get(format!("{base_url}/models"))
            .query(&[("key", &api_key)])
            .timeout(CONNECT_TIMEOUT)
            .send()
            .map_err(GeminiError::Unreachable)?;
        if !response.status().is_success() {
            return Err(GeminiError::UnexpectedStatus(response.status()));
        }
        let body: Value = response
            .json()
            .map_err(|e| GeminiError::ResponseParse(e.to_string()))?;

        let model_is_known = body
            .get("models")
            .and_then(Value::as_array)
            .map(|models| {
                models
                    .iter()
                    .filter_map(|entry| entry.get("name").and_then(Value::as_str))
                    .any(|name| name.strip_prefix("models/").unwrap_or(name) == model)
            })
            .unwrap_or(false);
        if !model_is_known {
            eprintln!(
                "warning: {base_url} didn't list {model:?} among its own real models -- \
                 continuing anyway; a genuinely wrong name will surface on the first real \
                 request instead"
            );
        }

        Ok(GeminiBackend {
            client,
            base_url,
            api_key,
            model,
        })
    }
}

impl InferenceBackend for GeminiBackend {
    /// A real HTTP round trip against Gemini's real `generateContent` endpoint, parsed loosely
    /// via [`serde_json::Value`] (fishing for `candidates[0].content.parts[0].text`) rather than
    /// a strict typed `Deserialize`. Every failure is embedded as `"[gemini backend error:
    /// ...]"` text, matching every other real backend's convention in this crate -- this
    /// trait's contract returns a plain `String`, never a `Result`. `cancel` is unused: one
    /// blocking HTTP call, no real per-chunk boundary -- see
    /// `hyperion_ai_runtime::CancellationToken`'s own doc comment.
    fn generate(
        &self,
        _model_id: u64,
        request: &InferenceRequest,
        _cancel: &CancellationToken,
    ) -> String {
        let payload = serde_json::json!({
            "contents": [{ "parts": [{ "text": request.prompt }] }],
        });

        let response = match self
            .client
            .post(format!(
                "{}/models/{}:generateContent",
                self.base_url, self.model
            ))
            .query(&[("key", &self.api_key)])
            .timeout(GENERATE_TIMEOUT)
            .json(&payload)
            .send()
        {
            Ok(response) => response,
            Err(e) => {
                return format!("[gemini backend error: couldn't reach the real Gemini API: {e}]")
            }
        };
        if !response.status().is_success() {
            return format!(
                "[gemini backend error: the real Gemini API responded with an unexpected \
                 status: {}]",
                response.status()
            );
        }
        let body: Value = match response.json() {
            Ok(body) => body,
            Err(e) => {
                return format!(
                    "[gemini backend error: couldn't parse the real Gemini API's response: {e}]"
                )
            }
        };

        body.get("candidates")
            .and_then(Value::as_array)
            .and_then(|candidates| candidates.first())
            .and_then(|candidate| candidate.get("content"))
            .and_then(|content| content.get("parts"))
            .and_then(Value::as_array)
            .and_then(|parts| parts.first())
            .and_then(|part| part.get("text"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                "[gemini backend error: the real Gemini API's response had no \
                 candidates[0].content.parts[0].text]"
                    .to_string()
            })
    }
}
