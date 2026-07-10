use hyperion_capability::TrustBoundaryId;

pub type PluginId = u64;
pub type CapabilityId = String;

/// docs/24 §Sandboxing's depth spectrum, reused as a **policy label** this
/// crate enforces against a manifest's declared minimum — not a new
/// isolation mechanism. Real sandbox strength is whatever
/// `hyperion-capability`'s Trust Boundary already provides; this crate
/// adds no second enforcement layer, per docs/24's own "no first-party
/// exemption, every plugin goes through the same admission check."
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustDepth {
    D0,
    D1,
    D2,
    D3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImplementationKind {
    LocalSmallModel,
    LocalLargeModel,
    CloudApi,
    NativeBinary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideEffect {
    CreatesSemanticObject,
    NetworkEgress,
    None,
}

/// docs/24 §4's `CapabilityGrantRequest.operation`, narrowed from the
/// doc's parameterized `Read(SemanticObjectClass)`/`NetworkEgress(Domain)`
/// to a flat enum — this crate's manifests carry the fine-grained scope
/// separately in `CapabilityGrantRequest.scope`. `Hash` is derived so
/// `hyperion-sdk`'s publish-time permission-set diffing can key a
/// `HashSet<Operation>` directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operation {
    Read,
    Write,
    NetworkEgress,
    Execute,
}

#[derive(Debug, Clone)]
pub struct CapabilityGrantRequest {
    pub operation: Operation,
    pub scope: String,
    pub justification: String,
}

/// docs/24 §4's `SemanticContract`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticContract {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub side_effects: Vec<SideEffect>,
}

/// docs/24 §4's `CapabilityManifest`. `quality_score` stands in for the
/// doc's `quality_hooks: BenchmarkHarnessRef` — a real benchmark harness
/// doesn't exist in this workspace, so a publisher declares this value
/// directly, the same "caller supplies what a real harness would
/// measure" pattern this workspace uses throughout. This is the one
/// field [23 — Multi-Model Orchestration](../23-multi-model-orchestration.md)'s
/// Model Router actually needs to score competing implementations
/// against each other — without it, every registered implementation
/// would be indistinguishable to real quality-based selection.
#[derive(Debug, Clone)]
pub struct CapabilityManifest {
    pub capability_id: CapabilityId,
    pub contract: SemanticContract,
    pub implementation_kind: ImplementationKind,
    pub quality_score: f32,
    pub version: u32,
}

/// docs/24 §4's `Contribution`, narrowed to the one variant this
/// workspace has an owning subsystem for — see this crate's doc comment
/// on the other eight variants' deferral.
#[derive(Debug, Clone)]
pub enum Contribution {
    Capability(CapabilityManifest),
}

/// docs/24 §4's `PluginManifest`. `signature` is this crate's usual non-
/// cryptographic-checksum stand-in (the same pattern as
/// `hyperion-ai-runtime`'s `checksum`/`hyperion-security`'s model
/// integrity check).
#[derive(Debug, Clone)]
pub struct PluginManifest {
    pub plugin_id: PluginId,
    pub publisher: String,
    pub signature: u64,
    pub sdk_version: u32,
    pub contributions: Vec<Contribution>,
    pub requested_permissions: Vec<CapabilityGrantRequest>,
    pub min_trust_depth: TrustDepth,
}

/// docs/24 §4's `RegistryEntry.install_state`. No separate "Disabled"/
/// "Uninstalled" state: uninstall is an action
/// ([`crate::registry::PluginRegistry::uninstall`]) that removes the
/// entry's contributions, not a fifth state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallState {
    Pending,
    Active,
    Quarantined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuarantineReason {
    PermissionOverreach,
    IntegrityFailure,
    PolicyViolation,
}

/// docs/24 §6's `ImplementationDescriptor` — the thing
/// [23 — Multi-Model Orchestration](../23-multi-model-orchestration.md)'s
/// Model Router selects between. `quality_score` stands in for
/// `quality_hooks`'s real benchmark harness result.
#[derive(Debug, Clone)]
pub struct ImplementationDescriptor {
    pub plugin_id: PluginId,
    pub implementation_kind: ImplementationKind,
    pub quality_score: f32,
    pub version: u32,
}

/// docs/24 §4's `RegistryEntry`, with `contract` added (not in the doc's
/// literal field list) so [`crate::registry::PluginRegistry`]'s
/// structural-compatibility check has something to compare a colliding
/// `capability_id`'s new contribution against without re-deriving it from
/// `implementations`.
#[derive(Debug, Clone)]
pub struct RegistryEntry {
    pub capability_id: CapabilityId,
    pub contract: SemanticContract,
    pub implementations: Vec<ImplementationDescriptor>,
    pub owning_plugins: Vec<PluginId>,
    pub install_state: InstallState,
}

pub struct PluginHandle {
    pub plugin_id: PluginId,
    pub boundary: TrustBoundaryId,
}

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("manifest signature does not verify")]
    SignatureInvalid,
    #[error("requested permission '{0:?}' is not justified by any declared side effect")]
    PermissionOverreach(Operation),
    #[error("this environment cannot satisfy the manifest's declared minimum trust depth")]
    InsufficientTrustDepth,
    #[error("installation was not consented to")]
    ConsentDeclined,
    #[error("capability_id collides with an existing, structurally incompatible contract")]
    CapabilityCollisionIncompatible,
    #[error("no such plugin")]
    NoSuchPlugin,
    #[error("no such capability_id in the registry")]
    NoSuchCapability,
    #[error("capability fault: {0}")]
    Capability(#[from] hyperion_capability::Fault),
}
