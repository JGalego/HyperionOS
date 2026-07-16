//! Hyperion L1/L4 Local AI Runtime — Phase 3, first slice.
//!
//! Implements docs/22-local-ai-runtime.md's own scope precisely: "given
//! 'run local model M, class C,' at what precision and residency state does
//! it execute on *this* hardware, *right now*" — never *which* model/
//! implementation satisfies a Capability (that is
//! [23 — Multi-Model Orchestration](../23-multi-model-orchestration.md)'s
//! job, built as `hyperion-model-router` in this same phase) and never
//! *whether* execution may leave the device ([16 — Privacy
//! Architecture](../16-privacy-architecture.md), Phase 8).
//!
//! Per this workspace's hosted-simulator convention, `runtime.infer`
//! executes against a pluggable [`InferenceBackend`] trait with a
//! deterministic [`MockBackend`] — no real model weights, no real forward
//! pass, by default. What *is* real: hardware-adaptive tier selection with step-down
//! retry (docs/22 §5.1), LRU-by-value residency management with pinning
//! (§5.2), and capability-gated, cancellation-safe invocation.
//!
//! docs/998-roadmap.md M8 adds exactly the swap this crate's own doc comment already
//! anticipated: behind the `candle` Cargo feature (off by default -- see
//! [`candle_backend`]'s own doc comment for why), [`candle_backend::CandleBackend`] is a real
//! [`InferenceBackend`] running a real, small Candle-loaded model on CPU. `MockBackend` remains
//! the default for every existing test and every caller that doesn't opt in.
//!
//! "Phase 1: local-engine backends" adds a second, independent real backend: behind the
//! `openai-compat` Cargo feature (also off by default), [`openai_compat_backend::
//! OpenAiCompatBackend`] speaks the OpenAI-compatible `/v1/chat/completions` REST shape any
//! Ollama/vLLM/self-hosted-LiteLLM server exposes -- one implementation, parameterized by
//! `base_url`/`model`, standing in for in-process Candle when a real local engine is preferred.
//!
//! "Phase 2: cloud providers" adds real OpenAI (a preset atop the same `OpenAiCompatBackend`,
//! since OpenAI's own API already speaks that same shape) plus two genuinely new backends behind
//! their own Cargo features, [`anthropic_backend::AnthropicBackend`] and
//! [`gemini_backend::GeminiBackend`], for the two providers whose real wire protocols aren't
//! OpenAI-shaped. Gated behind a real, console-level user consent -- see
//! `hyperion-console::ConsoleSession`'s own docs on the capability-gated dispatch that enforces
//! it -- since these send real data to a real, paid, external service.
//!
//! Deliberately deferred, and why:
//!
//! - **Real model execution** is no longer fully deferred -- see the M8 note above -- but
//!   reaching docs/36's actual 1-3B-parameter "small resident" production tier, on real
//!   NPU/GPU-accelerated reference hardware within its stated latency budget, is: this crate's
//!   real backend runs a genuinely tiny (15M-parameter) checkpoint on CPU only, proving the
//!   mechanism, not the production-scale target (see [`candle_backend`]'s own doc comment).
//! - **Scheduler governor integration (§5.3).** Real integration would
//!   subscribe to `hyperion-scheduler`'s `ResourceLedger.capacity` scaling;
//!   this crate instead takes a caller-supplied [`PowerMode`] directly via
//!   [`LocalAiRuntime::set_power_mode`], modeling the *consequence* of a
//!   governor tick (fewer concurrent streams, forced downgrade) without
//!   wiring the actual feedback loop — that wiring belongs to whichever
//!   later phase first has a real caller on both ends.
//! - ~~**Cancellable streaming (§Data Structures' `TokenStream`)**~~ — now real: a caller-visible
//!   `request_id` (via [`LocalAiRuntime::infer_cancellable`]) registers a real
//!   [`runtime::CancellationToken`] in a real `in_flight` registry, and [`LocalAiRuntime::cancel`]
//!   flips it for real rather than being the previous no-op stub. [`candle_backend::CandleBackend`]
//!   is the one real backend with a genuine per-token loop to check it at; every HTTP-backed
//!   backend receives the token but can't act on it mid-call (one blocking round trip, no
//!   per-chunk boundary) — see [`runtime::CancellationToken`]'s own doc comment for the honest
//!   split.
//! - ~~**Real model-artifact signing** (§Security Considerations)~~ — now real
//!   (docs/998-roadmap.md M9): [`LocalAiRuntime::register_model`] checks a real Ed25519
//!   signature (via [`hyperion_crypto`]) over [`sign`]'s canonical bytes, not a non-cryptographic
//!   checksum a forger could reproduce without the real signing key. `hyperion-context`'s own
//!   envelope-integrity checksum is a separate, not-yet-touched stand-in of the same shape —
//!   named, not silently implied fixed by this crate's own upgrade.

#[cfg(feature = "anthropic")]
pub mod anthropic_backend;
#[cfg(feature = "candle")]
pub mod candle_backend;
#[cfg(feature = "gemini")]
pub mod gemini_backend;
#[cfg(feature = "openai-compat")]
pub mod openai_compat_backend;
mod registry;
mod residency;
mod runtime;
mod types;

#[cfg(feature = "anthropic")]
pub use anthropic_backend::{AnthropicBackend, AnthropicError};
#[cfg(feature = "candle")]
pub use candle_backend::{CandleBackend, CandleBackendError};
#[cfg(feature = "gemini")]
pub use gemini_backend::{GeminiBackend, GeminiError};
#[cfg(feature = "openai-compat")]
pub use openai_compat_backend::{OpenAiCompatBackend, OpenAiCompatError};
pub use registry::{sign, verify, MockBackend};
pub use runtime::{CancellationToken, InferenceBackend, LocalAiRuntime, RuntimeError};
pub use types::{
    CapabilityContract, InferenceRequest, InferenceResult, ModelClass, ModelDescriptor, PowerMode,
    Precision, QuantizedVariant, ResidencyEntry, ResidencyStatus, ResourceEstimate,
};
