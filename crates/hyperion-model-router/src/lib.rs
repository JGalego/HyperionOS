//! Hyperion L4 Multi-Model Orchestration — Phase 3, third slice.
//!
//! Implements docs/23-multi-model-orchestration.md's Model Router: given a
//! Capability invocation, decide *which implementation* satisfies it — a
//! decision, never an execution. [22 — Local AI
//! Runtime](../22-local-ai-runtime.md) (`hyperion-ai-runtime`, already
//! built) owns *how* a chosen local model actually runs; this crate never
//! runs one, it only asks `runtime.estimate()` whether one could.
//!
//! Per docs/41-implementation-phases.md's own Phase 3 scope note — "single-
//! model routing scaffold only, full ensemble/fallback logic matures in
//! Phase 9" — this crate implements the *complete* candidate-gathering →
//! privacy-gate → feasibility-gate → weighted-scoring → fallback-chain
//! pipeline (docs/23 §Algorithms 1-4), with a real circuit breaker
//! (§Recovery Mechanisms) and a real Shadow/Canary/GA rollout-stage filter
//! (§Algorithms 1/6). What's deliberately *not* here, and why:
//!
//! - **Ensemble/verification dispatch** (§Algorithms 5, §Architecture's
//!   "Ensemble / verification pattern") — running two architecturally
//!   distinct implementations in parallel and reconciling agreement/
//!   disagreement needs real invokable implementations
//!   ([11 — Agent Runtime](../11-agent-runtime.md), Phase 4) to dispatch
//!   to; this crate's `route()` still computes whether ensemble
//!   verification *would* be needed (`needs_verification`) and reports it
//!   in the [`Rationale`], but never actually invokes a second candidate.
//! - **Real Capability Registry** ([24 — Plugin Framework](../24-plugin-framework.md),
//!   Phase 9) — candidates are registered directly via
//!   [`ModelRouter::register_implementation`] rather than discovered from a
//!   real plugin registry, and registration is not capability-gated here:
//!   Plugin Framework is what actually owns the Trust Boundary a real
//!   "install/register an implementation" crossing would check against.
//!   `route()` itself crosses no Trust Boundary — it is a pure decision
//!   over already-visible registry data plus `hyperion-ai-runtime`'s
//!   ungated `estimate()` — so, unlike every crate in this workspace that
//!   actually reads or writes a capability-scoped resource, this one has
//!   no capability check to perform yet.
//! - **Real privacy-tier policy** ([16 — Privacy Architecture](../16-privacy-architecture.md),
//!   Phase 8) — the privacy gate here is real and hard (a `CloudApi`
//!   candidate is unconditionally excluded without
//!   `CapabilityInvocation.cloud_consent`), but the *policy* deciding what
//!   counts as consent is a single caller-supplied bool, not 16's real
//!   per-data-class consent model.
//! - **Percentage-based canary traffic splitting** — `RolloutStage::Canary`
//!   is tracked and lightly discounted in scoring, but no random sampling
//!   actually splits live traffic by percentage; that needs
//!   [32 — Update System](../32-update-system.md) (Phase 9/10).
//! - **Durable decision log / `get_rationale` by id** ([34 — Observability
//!   & Telemetry](../34-observability-telemetry.md), Phase 8) — every
//!   [`RoutingDecision`] already carries its full [`Rationale`] inline;
//!   there is no separate persisted lookup-by-`invocation_id` yet.

mod registry;
mod router;
mod types;

pub use registry::ImplementationRegistry;
pub use router::ModelRouter;
pub use types::{
    CapabilityInvocation, ConsequenceTier, CostModel, ExclusionReason, ImplId, ImplKind,
    ImplementationDescriptor, PrivacyTier, Rationale, RolloutStage, RoutingDecision, RoutingScore,
    UrgencyClass,
};
