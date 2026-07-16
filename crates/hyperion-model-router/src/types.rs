use std::collections::HashMap;

use hyperion_ai_runtime::ModelClass;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImplId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImplKind {
    LocalSmallModel,
    LocalLargeModel,
    CloudApi,
    NativeBinary,
    Composed,
}

/// docs/23 §Data Structures' action-severity axis — independent of a
/// Capability's *provenance* trust (who published/vetted the code, which
/// docs/23 attributes to [15 — Security Architecture](../15-security-architecture.md),
/// not built yet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConsequenceTier {
    Routine,
    Sensitive,
    HighStakes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrgencyClass {
    Interactive,
    Background,
    Batch,
}

/// A narrowed stand-in for docs/16's real privacy-tier taxonomy — see this
/// crate's doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyTier {
    Local,
    ConsentedCloud,
}

#[derive(Debug, Clone, Copy)]
pub enum CostModel {
    Free,
    PerCall(f64),
    PerToken(f64),
}

/// docs/23 §Data Structures' `RolloutStage`, now with the real `Canary(f32)` traffic-percentage
/// payload docs/23 asks for. The `f32` is the real fraction (`0.0..=1.0`) of live invocations
/// this candidate is even eligible to compete for on a given call — the rest of live traffic
/// never considers it that call, falling straight through to whatever GA (or other in-sample
/// Canary) candidates already exist, docs/23's own "existing fallback chain still live as a
/// safety net" made real rather than a flat, uniform score discount applied to every single call
/// regardless of the declared percentage. See [`crate::router::ModelRouter::route`]'s own doc
/// comment for the real, deterministic sampling this drives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RolloutStage {
    Shadow,
    Canary(f32),
    Ga,
}

/// A narrowed, local copy of `hyperion_scheduler::ResourceVector`'s shape — this crate's own
/// declared "what would admitting this implementation cost the Scheduler" axis. Deliberately not
/// a dependency on that crate: `hyperion-scheduler` is the one that depends on
/// `hyperion-model-router` (to ask "is there a cheaper registered implementation for this
/// capability"), so a reverse dependency here would cycle. `hyperion-scheduler`'s own
/// model-tier-degradation caller converts between the two shapes field-for-field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourceCost {
    pub cpu_shares: u32,
    pub ram_mb: u32,
    pub gpu_shares: u32,
    pub vram_mb: u32,
    pub storage_iops: u32,
    pub network_bw_kbps: u32,
    pub inference_tokens_per_sec: u32,
    pub context_window_slots: u32,
    pub battery_budget_mw: u32,
}

/// docs/23 §Data Structures' `ImplementationDescriptor`, narrowed per this
/// crate's doc comment (no `trust_level`/`owning_plugin` — Plugin Framework
/// doesn't exist yet).
#[derive(Debug, Clone)]
pub struct ImplementationDescriptor {
    pub impl_id: ImplId,
    pub capability_id: String,
    pub kind: ImplKind,
    pub model_class: Option<ModelClass>,
    pub privacy_tier: PrivacyTier,
    pub cost_model: CostModel,
    /// `task_class -> quality`, keyed by `capability_id` as a stand-in for
    /// a real task-class taxonomy — see this crate's doc comment.
    pub quality_profile: HashMap<String, f32>,
    /// Declared p50 latency for non-local-model kinds
    /// (`CloudApi`/`NativeBinary`/`Composed`); local model kinds are
    /// estimated via `hyperion-ai-runtime` instead.
    pub declared_latency_ms: u64,
    pub rollout_stage: RolloutStage,
    /// `hyperion-scheduler`'s own named "model-tier degradation" gap: what this implementation
    /// would cost the real Scheduler admission ledgers, if known. `None` for implementations that
    /// never draw against the Scheduler at all (e.g. a `CloudApi` candidate whose cost is purely
    /// `cost_model`, not local resource contention) — honest absence, not zero cost.
    pub resource_cost: Option<ResourceCost>,
}

/// docs/23 §Data Structures' `CapabilityInvocation`, narrowed to what this
/// scaffold's scoring pipeline actually consumes.
#[derive(Debug, Clone)]
pub struct CapabilityInvocation {
    pub capability_id: String,
    pub urgency_class: UrgencyClass,
    pub consequence_tier: ConsequenceTier,
    pub quality_floor: Option<f32>,
    pub latency_budget_ms: u64,
    /// A stand-in for a Context Bundle carrying a current, explicit consent
    /// record from [16 — Privacy Architecture](../16-privacy-architecture.md)
    /// — see this crate's doc comment.
    pub cloud_consent: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct RoutingScore {
    pub impl_id: ImplId,
    pub latency_fit: f32,
    pub privacy_fit: f32,
    pub cost_fit: f32,
    pub quality_fit: f32,
    pub availability_fit: f32,
    pub composite: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExclusionReason {
    PrivacyGate,
    ResourceInfeasible,
    /// This call's real, deterministic traffic sample landed outside a `RolloutStage::Canary`
    /// candidate's own declared percentage — unlike `Shadow` (never a candidate at all, scored
    /// only via a separate path), a `Canary` candidate genuinely is a candidate this cycle, it
    /// just didn't get real traffic sampled its way this particular call.
    CanaryNotSampled,
}

#[derive(Debug, Clone)]
pub struct Rationale {
    pub candidates_considered: Vec<(ImplId, RoutingScore)>,
    pub candidates_excluded: Vec<(ImplId, ExclusionReason)>,
    pub chosen_reason: String,
    /// docs/23 §Algorithms 5: would this invocation trigger ensemble
    /// verification if this crate dispatched one — see this crate's doc
    /// comment on the deferred dispatch itself.
    pub needs_verification: bool,
}

#[derive(Debug, Clone)]
pub struct RoutingDecision {
    pub invocation_id: u64,
    pub chosen: Option<ImplId>,
    pub fallback_chain: Vec<ImplId>,
    pub rationale: Rationale,
}
