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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
///
/// `CheaperLocalTier`/`AlternateImplementation` each carry a real
/// [`CapacityDescriptor`] — this crate's own previously-named "`Substitution` carries no
/// resource footprint" gap, closed for real: whoever declares a fallback already knows what it
/// costs, so the footprint travels with the declaration itself rather than requiring a caller to
/// separately maintain an out-of-band `ModelTier`/`CapabilityRef` -> `CapacityDescriptor` lookup
/// table. [`Substitution::footprint`] is the one real accessor [`crate::fit::scheduler_backed_resolver`]
/// now uses instead of taking a caller-supplied `footprint_for` closure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Substitution {
    CheaperLocalTier(ModelTier, CapacityDescriptor),
    AlternateImplementation(CapabilityRef, CapacityDescriptor),
    ConsentedCloudUpgrade(String),
    Disable,
}

impl Substitution {
    /// This substitution's own real resource footprint, if it has one --
    /// `ConsentedCloudUpgrade`/`Disable` name no local resource cost at all (a cloud upgrade's
    /// footprint is the *remote* provider's problem, not this device's; disabling costs nothing
    /// by definition), so `None` for those is a real, honest absence, not an oversight.
    pub fn footprint(&self) -> Option<CapacityDescriptor> {
        match self {
            Substitution::CheaperLocalTier(_, footprint) => Some(*footprint),
            Substitution::AlternateImplementation(_, footprint) => Some(*footprint),
            Substitution::ConsentedCloudUpgrade(_) | Substitution::Disable => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn footprint() -> CapacityDescriptor {
        CapacityDescriptor {
            ram_mb: 1_024,
            vram_mb: 512,
            compute_tops: 3,
        }
    }

    #[test]
    fn cheaper_local_tier_and_alternate_implementation_carry_their_own_real_footprint() {
        assert_eq!(
            Substitution::CheaperLocalTier(ModelTier::TinyEdge, footprint()).footprint(),
            Some(footprint())
        );
        assert_eq!(
            Substitution::AlternateImplementation("web.search.small".to_string(), footprint())
                .footprint(),
            Some(footprint())
        );
    }

    #[test]
    fn consented_cloud_upgrade_and_disable_have_no_local_footprint() {
        assert_eq!(
            Substitution::ConsentedCloudUpgrade("acme-cloud".to_string()).footprint(),
            None
        );
        assert_eq!(Substitution::Disable.footprint(), None);
    }
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
    /// [`crate::apply_and_explain`]'s real-registry check: an
    /// `AlternateImplementation` substitution named a capability that
    /// isn't actually installed (or is quarantined) in the real
    /// `hyperion-plugin-framework` registry it was checked against —
    /// never write an audit notice claiming a fallback happened when it
    /// didn't.
    #[error("alternate implementation {0:?} is not a real, active registered capability")]
    AlternateImplementationNotRegistered(CapabilityRef),
}
