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
//! - ~~**Percentage-based canary traffic splitting**~~ — now real:
//!   [`types::RolloutStage::Canary`] carries a real `f32` traffic-percentage payload, and
//!   [`router::ModelRouter::route`] really samples it — deterministically, keyed on the real
//!   `invocation_id` and `impl_id`, via a real hash rather than a caller-supplied RNG — so only
//!   that declared fraction of live calls even consider the candidate; the rest fall straight
//!   through to whatever GA (or other in-sample Canary) candidate already exists, docs/23's own
//!   "existing fallback chain still live as a safety net." A candidate that *is* sampled in still
//!   carries the same modest `availability_fit` discount this crate always applied to Canary —
//!   the two mechanisms are additive, not a replacement of one by the other. What's still
//!   deferred: *deciding* what percentage to declare and when to ratchet it up over a real
//!   rollout's lifetime remains [32 — Update System](../32-update-system.md)'s own job (Phase
//!   9/10) — this crate only makes an already-declared percentage real, it doesn't decide one.
//! - ~~Durable decision log~~ — now real: `hyperion-api-gateway`'s
//!   `invoke_capability` appends every real routing decision's
//!   [`Rationale`] to `hyperion-observability::AuditLedger` via the new
//!   `AuditPayload::ModelRouting` variant, giving it a durable, queryable
//!   log. `get_rationale`-by-`invocation_id` specifically is still not a
//!   dedicated index — the ledger's own `query`/`seq` lookup is by
//!   `target` (the capability id) and sequence, not `invocation_id`.

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
