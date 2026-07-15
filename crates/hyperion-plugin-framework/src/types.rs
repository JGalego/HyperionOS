use std::path::PathBuf;

use hyperion_capability::TrustBoundaryId;

pub type PluginId = u64;
pub type CapabilityId = String;

/// The real, previously-missing "something for a sandbox to run" this crate's own doc comment
/// names: `program` + `args` are handed straight to `hyperion_trust_boundary::spawn` (via
/// `std::process::Command`), never interpreted or parsed by this crate. Populated only for
/// `ImplementationKind::NativeBinary` -- every other kind still has no execution story, exactly
/// as before.
#[derive(Debug, Clone)]
pub struct NativeBinaryDescriptor {
    pub program: PathBuf,
    pub args: Vec<String>,
}

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
    /// Real execution info for `ImplementationKind::NativeBinary` -- `None` for every other kind
    /// (they each already have their own real dispatch elsewhere: a local/cloud model backend, a
    /// cloud API client). `PluginRegistry::install` rejects a `NativeBinary` manifest that leaves
    /// this `None`, or whose `program` doesn't really exist and isn't really executable -- an
    /// honest check at install time, not a trusted claim.
    pub native_binary: Option<NativeBinaryDescriptor>,
}

/// A plugin-contributed agent specialization's own manifest fields — mirrors
/// `hyperion_agent_runtime::AgentManifest`'s shape without this crate depending on that crate
/// (which already depends on this one; a reverse dependency would cycle). Real trust tier for a
/// plugin-installed agent is always the least-trusted tier, assigned by
/// `crate::registry::PluginRegistry::agent_contributions`'s caller, never chosen by the
/// installer — no publisher-key trust store exists yet to justify anything higher (see this
/// crate's own doc comment on `hyperion_crypto`'s single-device-identity scope).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentContribution {
    pub specialization: String,
    pub baseline_capabilities: Vec<String>,
    pub requestable_capabilities: Vec<String>,
}

/// docs/24 §4's `Contribution`, narrowed to the two variants this
/// workspace has an owning subsystem for — see this crate's doc comment
/// on the remaining six variants' deferral. `Agent` closes
/// docs/998-roadmap.md's own "`hyperion-coordination::catalog::default_manifests` is a
/// hardcoded, static built-in list, not a live registry a plugin's `AgentManifest` could
/// register into" gap: `crate::registry::PluginRegistry::agent_contributions` is that live
/// registry.
#[derive(Debug, Clone)]
pub enum Contribution {
    Capability(CapabilityManifest),
    Agent(AgentContribution),
}

/// docs/24 §4's `PluginManifest`. `signature` (docs/998-roadmap.md M9) is a real Ed25519
/// signature over [`crate::review::sign`]'s canonical bytes — see that function's own doc
/// comment on this workspace's single-device-identity model. `None` until a caller signs it.
#[derive(Debug, Clone)]
pub struct PluginManifest {
    pub plugin_id: PluginId,
    pub publisher: String,
    pub signature: Option<hyperion_crypto::Signature>,
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
    /// Carried straight over from the installing [`CapabilityManifest`]'s own field -- see there
    /// for why. [`crate::registry::PluginRegistry::invoke_native_binary`] is the real caller.
    pub native_binary: Option<NativeBinaryDescriptor>,
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

#[derive(Debug)]
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
    #[error("no runnable (NativeBinary) implementation is installed for '{0}'")]
    NoRunnableImplementation(CapabilityId),
    #[error("invalid NativeBinary implementation: {0}")]
    InvalidNativeBinary(String),
    #[error("sandboxed execution failed: {0}")]
    ExecutionFailed(String),
    #[error("capability fault: {0}")]
    Capability(#[from] hyperion_capability::Fault),
}
