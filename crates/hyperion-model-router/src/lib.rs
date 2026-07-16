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
//! - ~~**Ensemble/verification dispatch**~~ (§Algorithms 5, §Architecture's "Ensemble /
//!   verification pattern") — now real, but deliberately not in this crate:
//!   `hyperion-api-gateway::ApiGateway::verify_with_ensemble` is the real dispatch, since this
//!   crate's own architecture is "a decision, never an execution" (see this crate's own opening
//!   paragraph) — actually invoking a second candidate belongs where invocation already happens.
//!   `route()`'s own real contribution is unchanged: it still computes whether ensemble
//!   verification *would* be needed (`needs_verification`) and reports it in the [`Rationale`];
//!   the gateway is the real production caller that acts on that signal, dispatching a real,
//!   architecturally distinct second candidate (different [`types::ImplKind`]) and reconciling
//!   agreement (a real, boosted confidence) or disagreement (escalated, never silently resolved —
//!   this crate has no `designated_tiebreaker` concept for a gateway-level reconciler to consult).
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
//!   the two mechanisms are additive, not a replacement of one by the other. ~~*Deciding* what
//!   percentage to declare and when to ratchet it up~~ (2026-07-16) — now real too, and
//!   deliberately not here: `hyperion-update::UpdateOrchestrator::apply_update_with_rollout` is
//!   the real [32 — Update System](../32-update-system.md) caller docs/23 always named as owning
//!   this decision, calling `set_rollout_stage` with each real, health-gated stage's own real
//!   percentage — GA once every stage passes, demoted back to `Shadow` (never left stuck at a
//!   partial percentage) on a health breach. This crate's own contribution is still exactly
//!   `route()`/`canary_sampled_in` — it makes an already-declared percentage real, it never
//!   decides one itself.
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
    ImplementationDescriptor, PrivacyTier, Rationale, ResourceCost, RolloutStage, RoutingDecision,
    RoutingScore, UrgencyClass,
};
