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
//! pass. What *is* real: hardware-adaptive tier selection with step-down
//! retry (docs/22 §5.1), LRU-by-value residency management with pinning
//! (§5.2), and capability-gated, cancellation-safe invocation.
//!
//! Deliberately deferred, and why:
//!
//! - **Real model execution.** No ONNX/GGUF runtime, no real quantization —
//!   [`MockBackend`] returns a deterministic, content-derived string.
//!   Swapping in a real backend later only requires a new
//!   [`InferenceBackend`] impl; nothing else in this crate changes.
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

mod registry;
mod residency;
mod runtime;
mod types;

pub use registry::{checksum, MockBackend};
pub use runtime::{InferenceBackend, LocalAiRuntime, RuntimeError};
pub use types::{
    CapabilityContract, InferenceRequest, InferenceResult, ModelClass, ModelDescriptor, PowerMode,
    Precision, QuantizedVariant, ResidencyEntry, ResidencyStatus, ResourceEstimate,
};
