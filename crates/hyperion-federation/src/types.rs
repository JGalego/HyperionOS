use hyperion_scheduler::ResourceVector;

/// docs/21 §Data Structures' `DeviceRecord.trust_tier`. Declaration order
/// is *not* trust order here — see [`FederationHub`]'s tie-break, which
/// ranks by actual trust (`OwnedPrimary` most trusted), not discriminant
/// value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FederationTrustTier {
    OwnedPrimary,
    OwnedSecondary,
    SharedHousehold,
    CloudRented,
}

impl FederationTrustTier {
    /// Higher is more trusted — used by the lease split-brain tie-break
    /// (docs/21 §Recovery Mechanisms) and the cloud consent gate
    /// (§Security Considerations).
    pub(crate) fn trust_rank(self) -> u8 {
        match self {
            FederationTrustTier::OwnedPrimary => 3,
            FederationTrustTier::OwnedSecondary => 2,
            FederationTrustTier::SharedHousehold => 1,
            FederationTrustTier::CloudRented => 0,
        }
    }

    pub fn is_cloud(self) -> bool {
        matches!(self, FederationTrustTier::CloudRented)
    }
}

/// A narrowed stand-in for docs/16's real privacy-tier taxonomy — the same
/// simplification `hyperion-model-router`'s `PrivacyTier` already makes for
/// its own, unrelated gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyTier {
    Local,
    ConsentedCloud,
}

/// docs/21 §Data Structures' `VirtualResourceLedger`, narrowed to a single
/// `ResourceVector` snapshot of available headroom rather than a per-
/// dimension `ResourceLedger` — still `hyperion-scheduler`'s real struct,
/// unmodified, per the doc's own "consumed... as one more ResourceLedger."
#[derive(Debug, Clone, Copy)]
pub struct VirtualResourceLedger {
    pub device_id: u64,
    pub trust_tier: FederationTrustTier,
    pub available: ResourceVector,
    pub network_latency_ms: u32,
    pub published_at: u64,
    pub ttl_secs: u64,
}

impl VirtualResourceLedger {
    pub(crate) fn is_live(&self, now: u64) -> bool {
        now.saturating_sub(self.published_at) <= self.ttl_secs
    }
}

/// docs/21 §Data Structures' `OffloadDescriptor`, narrowed to what
/// [`FederationHub::dispatch_offload`] actually scores against.
#[derive(Debug, Clone)]
pub struct OffloadDescriptor {
    pub request: ResourceVector,
    pub deadline_ms: Option<u32>,
    pub privacy_tier: PrivacyTier,
}

/// docs/21 §Data Structures' `AnchorLease`.
#[derive(Debug, Clone, Copy)]
pub struct AnchorLease {
    pub agent_instance: u64,
    pub holder_device: u64,
    pub generation: u64,
    pub granted_at: u64,
    pub ttl_secs: u64,
}

impl AnchorLease {
    pub(crate) fn is_live(&self, now: u64) -> bool {
        now.saturating_sub(self.granted_at) <= self.ttl_secs
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationOutcome {
    Completed,
    Failed,
}

/// docs/21 §Interfaces' `MigrationReceipt`.
#[derive(Debug, Clone, Copy)]
pub struct MigrationReceipt {
    pub migration_id: u64,
    pub agent_instance: u64,
    pub target_device: u64,
    pub outcome: MigrationOutcome,
}
