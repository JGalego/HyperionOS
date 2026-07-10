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

/// docs/23 §Data Structures' `RolloutStage`, without the `Canary(f32)`
/// traffic-percentage payload — see this crate's doc comment on deferred
/// percentage-based splitting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RolloutStage {
    Shadow,
    Canary,
    Ga,
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
