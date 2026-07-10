/// docs/37 §Performance Analysis's four-tier table, declared in
/// ascending capability order so the derived `Ord` matches "SBC is the
/// floor, EnterpriseNode is the ceiling."
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HardwareTier {
    Sbc,
    Laptop,
    Workstation,
    EnterpriseNode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenancyMode {
    SingleUser,
    MultiUserShared,
    MultiTenantOrg,
}

/// docs/37 §2's `CapacityDescriptor`, narrowed to the three dimensions
/// the tier table and `ResourceConstraint` actually key on — this
/// crate's own resource shape, deliberately not `hyperion_scheduler::ResourceVector`
/// (see this crate's doc comment on why forcing parity with that real
/// 9-dimension vector was judged not worth the friction for the three
/// dimensions docs/37 itself discusses: RAM, VRAM, compute).
#[derive(Debug, Clone, Copy)]
pub struct CapacityDescriptor {
    pub ram_mb: u32,
    pub vram_mb: u32,
    pub compute_tops: u32,
}

/// docs/37 §2's `HardwareProfile`.
#[derive(Debug, Clone, Copy)]
pub struct HardwareProfile {
    pub tier: HardwareTier,
    pub compute: CapacityDescriptor,
    pub tenancy: TenancyMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTier {
    TinyEdge,
    SmallResident,
    LargeLocal,
    Vision,
    SpeechAsr,
    SpeechTts,
}

pub type CapabilityRef = String;

/// docs/37 §3's `ResourceConstraint`.
#[derive(Debug, Clone, Copy)]
pub struct ResourceConstraint {
    pub min_ram_mb: u32,
    pub min_vram_mb: u32,
    pub min_compute_tops: u32,
}

impl ResourceConstraint {
    pub fn violated_by(&self, profile: &HardwareProfile) -> bool {
        profile.compute.ram_mb < self.min_ram_mb
            || profile.compute.vram_mb < self.min_vram_mb
            || profile.compute.compute_tops < self.min_compute_tops
    }
}

/// docs/37 §3's `Substitution` — note there is no variant that touches
/// [15 — Security Architecture](../15-security-architecture.md); "security
/// policy is never a substitution target," true here by construction,
/// not by a runtime check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Substitution {
    CheaperLocalTier(ModelTier),
    AlternateImplementation(CapabilityRef),
    ConsentedCloudUpgrade(String),
    Disable,
}

/// docs/37 §3's `DegradationPolicy`.
#[derive(Debug, Clone)]
pub struct DegradationPolicy {
    pub capability_ref: CapabilityRef,
    pub constraint: ResourceConstraint,
    /// Fixed evaluation order: cheaper local tier → alternate local
    /// implementation → consented cloud upgrade → disable — docs/37 §3's
    /// `degrade_capability` pseudocode never reorders this per call.
    pub fallback_order: Vec<Substitution>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DegradationOutcome {
    FullFidelity,
    Substituted { substitution: Substitution },
    Disabled,
}

/// docs/37 §4's `DegradationPlan`/`explain_degradation` result, merged
/// into one struct — `notice` is this crate's `ExplanationTemplate`
/// rendering, a deterministic `format!`, not real NLG (the same
/// downgrade this workspace's other crates already document).
#[derive(Debug, Clone)]
pub struct DegradationPlan {
    pub capability_ref: CapabilityRef,
    pub outcome: DegradationOutcome,
    pub notice: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ScalabilityError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("observability error: {0}")]
    Observability(#[from] hyperion_observability::ObservabilityError),
}
