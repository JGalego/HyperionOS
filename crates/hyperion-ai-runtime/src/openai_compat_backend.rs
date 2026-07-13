//! A real, feature-gated [`InferenceBackend`] speaking the OpenAI-compatible
//! `/v1/chat/completions` + `/v1/models` REST shape -- PRODUCTION_BOOT_PROMPT.md's "Phase 1:
//! local-engine backends."
//!
//! Ollama, vLLM, and a self-hosted LiteLLM proxy all speak (or can speak) this exact shape, so
//! one backend -- parameterized by `base_url` + `model` + an optional bearer key -- covers all
//! three (and OpenAI itself, later) rather than three bespoke clients. Only genuinely
//! non-OpenAI-shaped native APIs (Anthropic, Gemini) would need their own dedicated backend, a
//! deliberately separate, later phase.
//!
//! Gated behind the `openai-compat` Cargo feature, the same "off by default, real behind an
//! opt-in feature" convention [`crate::candle_backend`] already established: the default build
//! never pulls in `reqwest` or needs network access to build or test, and [`crate::MockBackend`]
//! remains every existing test's default.

use std::time::Duration;

use serde_json::Value;

use crate::runtime::InferenceBackend;
use crate::types::InferenceRequest;

/// Bounded, short: [`OpenAiCompatBackend::connect`]'s own real reachability proof shouldn't make
/// switching backends feel hung just because a configured server isn't actually running.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Deliberately far longer than [`CONNECT_TIMEOUT`] (and than
/// `hyperion_netstack::ReqwestFetchBackend`'s own 15s page-fetch timeout) -- a real chat
/// completion legitimately takes far longer than a page fetch, especially against a local,
/// CPU-bound engine.
const GENERATE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, thiserror::Error)]
pub enum OpenAiCompatError {
    #[error("couldn't build a real HTTP client: {0}")]
    ClientInit(#[source] reqwest::Error),
    #[error("couldn't reach {base_url} ({source})")]
    Unreachable {
        base_url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("{base_url} responded with an unexpected status: {status}")]
    UnexpectedStatus {
        base_url: String,
        status: reqwest::StatusCode,
    },
    #[error("couldn't parse {base_url}'s response: {reason}")]
    ResponseParse { base_url: String, reason: String },
}

/// A real backend for any OpenAI-compatible HTTP server -- Ollama, vLLM, a self-hosted LiteLLM
/// proxy, or OpenAI itself. [`Self::generate`] is the entire [`InferenceBackend`] contract: a
/// real prompt in, a real completion out, via a real HTTP round trip.
pub struct OpenAiCompatBackend {
    client: reqwest::blocking::Client,
    /// Never has a trailing slash (normalized in [`Self::connect`]) -- callers format
    /// `{base_url}/models`/`{base_url}/chat/completions` directly, and a stray trailing slash
    /// would otherwise silently produce a wrong, doubled-slash path.
    base_url: String,
    model: String,
    api_key: Option<String>,
}

impl OpenAiCompatBackend {
    /// Connects to a real OpenAI-compatible server and proves it's actually reachable via a
    /// real `GET {base_url}/models` call, eagerly, at construction time -- mirroring
    /// [`crate::candle_backend::CandleBackend::load`]'s own precedent of doing real work eagerly
    /// rather than deferring it. This matters more here than it did there: [`InferenceBackend::
    /// generate`] can't return a `Result` (it's the trait's own contract), so deferring this
    /// check would let a caller like the console's own backend-switch meta-command falsely
    /// report "Switched" and only ever surface the real failure garbled into the next answer.
    ///
    /// If `model` isn't present in the server's own returned model list, that's a soft,
    /// non-fatal warning, not a hard failure: some servers format ids unexpectedly (a local
    /// single-model vLLM deployment, for instance), and a hard failure here would be a false
    /// negative -- a genuinely wrong model name will surface naturally on the first real
    /// [`Self::generate`] call instead.
    pub fn connect(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: Option<String>,
    ) -> Result<Self, OpenAiCompatError> {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        let model = model.into();

        let client = reqwest::blocking::Client::builder()
            .build()
            .map_err(OpenAiCompatError::ClientInit)?;

        let mut request = client
            .get(format!("{base_url}/models"))
            .timeout(CONNECT_TIMEOUT);
        if let Some(key) = &api_key {
            request = request.bearer_auth(key);
        }
        let response = request
            .send()
            .map_err(|source| OpenAiCompatError::Unreachable {
                base_url: base_url.clone(),
                source,
            })?;
        if !response.status().is_success() {
            return Err(OpenAiCompatError::UnexpectedStatus {
                base_url,
                status: response.status(),
            });
        }
        let body: Value = response
            .json()
            .map_err(|e| OpenAiCompatError::ResponseParse {
                base_url: base_url.clone(),
                reason: e.to_string(),
            })?;

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
                 continuing anyway (some servers format model ids differently); a genuinely \
                 wrong name will surface on the first real request instead"
            );
        }

        Ok(OpenAiCompatBackend {
            client,
            base_url,
            model,
            api_key,
        })
    }
}

impl InferenceBackend for OpenAiCompatBackend {
    /// A real HTTP round trip: `POST {base_url}/chat/completions` with `request.prompt` as the
    /// one user message, parsed loosely via [`serde_json::Value`] rather than a strict typed
    /// `Deserialize` -- Ollama/vLLM/LiteLLM's exact response shapes drift slightly from each
    /// other and from OpenAI's own, and fishing for `choices[0].message.content` tolerates that.
    /// Every failure is embedded as `"[openai-compat backend error: ...]"` text, matching
    /// [`crate::candle_backend::CandleBackend::generate`]'s own convention -- this trait's
    /// contract returns a plain `String`, never a `Result`.
    fn generate(&self, _model_id: u64, request: &InferenceRequest) -> String {
        let payload = serde_json::json!({
            "model": self.model,
            "messages": [{ "role": "user", "content": request.prompt }],
        });

        let mut req = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .timeout(GENERATE_TIMEOUT)
            .json(&payload);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let response = match req.send() {
            Ok(response) => response,
            Err(e) => {
                return format!(
                    "[openai-compat backend error: couldn't reach {}: {e}]",
                    self.base_url
                )
            }
        };
        if !response.status().is_success() {
            return format!(
                "[openai-compat backend error: {} responded with an unexpected status: {}]",
                self.base_url,
                response.status()
            );
        }
        let body: Value = match response.json() {
            Ok(body) => body,
            Err(e) => {
                return format!(
                    "[openai-compat backend error: couldn't parse {}'s response: {e}]",
                    self.base_url
                )
            }
        };

        body.get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                format!(
                    "[openai-compat backend error: {}'s response had no \
                     choices[0].message.content]",
                    self.base_url
                )
            })
    }
}
