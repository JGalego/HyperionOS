use std::collections::HashSet;

use hyperion_knowledge_graph::NodeId;

/// docs/16 §3's three-tier policy setting — a **user/domain-level**
/// setting, distinct from [`SensitivityClass`] (an **object-level**
/// classification). Default recommended tier per docs/16 §12 is
/// `LocalPreferredWithConsent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrivacyTier {
    FullyLocal,
    LocalPreferredWithConsent,
    CloudAssisted,
}

/// docs/16 §3's four-value data classification, attached per-object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SensitivityClass {
    Public,
    Personal,
    Sensitive,
    Restricted,
}

/// docs/16 §4's `PrivacyProfile`, `Domain` left as an open `String` (a
/// capability/domain name) rather than a closed enum, matching this
/// workspace's own `hyperion-knowledge-graph::ObjectType` precedent for
/// "new values arrive without a schema migration."
#[derive(Debug, Clone)]
pub struct PrivacyProfile {
    pub tier: PrivacyTier,
    pub domain_overrides: std::collections::HashMap<String, PrivacyTier>,
    pub updated_at: u64,
    pub version: u32,
}

impl PrivacyProfile {
    pub fn tier_for(&self, domain: &str) -> PrivacyTier {
        self.domain_overrides
            .get(domain)
            .copied()
            .unwrap_or(self.tier)
    }
}

/// docs/16 §4's `ResidencyTag`, attached to a Semantic Object header.
/// `Restricted`-classified objects structurally cannot carry
/// `CloudAssisted` in `allowed_tiers` — enforced at construction, not
/// trusted to every call site to remember.
#[derive(Debug, Clone)]
pub struct ResidencyTag {
    pub object_id: NodeId,
    pub sensitivity: SensitivityClass,
    pub allowed_tiers: HashSet<PrivacyTier>,
}

impl ResidencyTag {
    pub fn new(
        object_id: NodeId,
        sensitivity: SensitivityClass,
        mut allowed_tiers: HashSet<PrivacyTier>,
    ) -> Self {
        if sensitivity == SensitivityClass::Restricted {
            allowed_tiers.remove(&PrivacyTier::CloudAssisted);
        }
        ResidencyTag {
            object_id,
            sensitivity,
            allowed_tiers,
        }
    }

    pub fn forbids(&self, tier: PrivacyTier) -> bool {
        !self.allowed_tiers.contains(&tier)
    }
}

/// docs/16 §4's `ConsentGrant.scope` — narrowed to the three shapes this
/// crate's callers actually need to name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DataScope {
    Domain(String),
    Object(NodeId),
    Capability(String),
}

/// docs/16 §4's `ConsentGrant` — `revocable` is always `true` per the doc
/// (not a field a caller could set `false`), and `proof: Signature` is
/// this crate's deferred real-crypto piece (see this crate's doc
/// comment).
#[derive(Debug, Clone)]
pub struct ConsentGrant {
    pub id: u64,
    pub subject: u64,
    pub scope: DataScope,
    pub purpose: String,
    pub expiry: Option<u64>,
    pub granted_at: u64,
}

impl ConsentGrant {
    pub(crate) fn is_live(&self, now: u64) -> bool {
        self.expiry.is_none_or(|e| now <= e)
    }
}

/// docs/16 §5's routing outcome — the result of
/// [`crate::routing::route_capability_call`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingDecision {
    DispatchLocal,
    DispatchCloud { grant_id: u64 },
    Degraded(DegradeReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradeReason {
    NoLocalImplementation,
    ResidencyForbidsCloud,
    NoStandingConsent,
}

/// docs/16 §5's `ErasureRequest.mode` — `CryptoShred` is this crate's
/// no-grace-period, no-recovery path; `SoftDelete` integrates with
/// [33 — Rollback & Recovery](../33-rollback-recovery.md)'s undo grace
/// period (not wired directly in this crate — see this crate's doc
/// comment).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErasureMode {
    SoftDelete,
    CryptoShred,
}

/// docs/16 §4's `ErasureReceipt`, narrowed to a single (this) device — no
/// `propagated_to_devices` field, since this crate has no multi-device
/// sync model to propagate across (see this crate's doc comment).
#[derive(Debug, Clone)]
pub struct ErasureReceipt {
    pub object_ids: Vec<NodeId>,
    pub mode: ErasureMode,
    pub completed_at: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum PrivacyError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("knowledge graph error: {0}")]
    Graph(#[from] hyperion_knowledge_graph::GraphError),
}
