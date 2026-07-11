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
//! PRODUCTION_BOOT_PROMPT.md M8 adds exactly the swap this crate's own doc comment already
//! anticipated: behind the `candle` Cargo feature (off by default -- see
//! [`candle_backend`]'s own doc comment for why), [`candle_backend::CandleBackend`] is a real
//! [`InferenceBackend`] running a real, small Candle-loaded model on CPU. `MockBackend` remains
//! the default for every existing test and every caller that doesn't opt in.
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
//! - **Cancellable streaming (§Data Structures' `TokenStream`).** Inference
//!   here is a single synchronous call, not a cancellable stream — there is
//!   no real generation loop to interrupt yet. `runtime.cancel` exists in
//!   the API surface but is a no-op stub, noted at its call site.
//! - **Real model-artifact signing** (§Security Considerations) — model
//!   registration checks a non-cryptographic checksum (the same
//!   fnv1a64-style stand-in `hyperion-context` already uses for envelope
//!   integrity), not a real signature; both wait on
//!   [15 — Security Architecture](../15-security-architecture.md) (Phase 8)
//!   for real key material.

#[cfg(feature = "candle")]
pub mod candle_backend;
mod registry;
mod residency;
mod runtime;
mod types;

#[cfg(feature = "candle")]
pub use candle_backend::{CandleBackend, CandleBackendError};
pub use registry::{checksum, MockBackend};
pub use runtime::{InferenceBackend, LocalAiRuntime, RuntimeError};
pub use types::{
    CapabilityContract, InferenceRequest, InferenceResult, ModelClass, ModelDescriptor, PowerMode,
    Precision, QuantizedVariant, ResidencyEntry, ResidencyStatus, ResourceEstimate,
};
