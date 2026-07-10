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
//! - ~~Real Capability Registry~~ — now real:
//!   `hyperion-api-gateway::router_bridge` discovers candidates from the
//!   actual `hyperion-plugin-framework` registry and bridges each into
//!   [`ModelRouter::register_implementation`], so a third-party Capability
//!   and a first-party equivalent genuinely compete on this crate's real
//!   weighted scoring. [`ModelRouter::register_implementation`]/
//!   [`ModelRouter::set_rollout_stage`] are also now capability-gated
//!   (`RightsMask::WRITE`, returning [`ModelRouterError`]) — the
//!   "not capability-gated here" gap this bullet used to name. `route()`
//!   itself still crosses no Trust Boundary of its own — it remains a
//!   pure decision over already-visible registry data plus
//!   `hyperion-ai-runtime`'s ungated `estimate()`.
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
pub use router::{ModelRouter, ModelRouterError};
pub use types::{
    CapabilityInvocation, ConsequenceTier, CostModel, ExclusionReason, ImplId, ImplKind,
    ImplementationDescriptor, PrivacyTier, Rationale, RolloutStage, RoutingDecision, RoutingScore,
    UrgencyClass,
};
