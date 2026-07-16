use std::path::PathBuf;

use hyperion_capability::TrustBoundaryId;
use hyperion_scheduler::ResourceVector;

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

/// A narrowed stand-in for docs/16's real privacy-tier taxonomy — the same simplification
/// `hyperion-model-router`'s own `PrivacyTier` (and `hyperion-federation`'s own copy) already
/// makes for its unrelated gate. Deliberately this crate's own local copy, not a dependency on
/// `hyperion-model-router`'s: that crate's own doc comment explicitly doesn't want a dependency
/// on the Plugin Framework, and this crate has no reason to know the Model Router's scoring shape
/// either — `hyperion-api-gateway::router_bridge` (which already depends on both) is the real
/// adapter that maps this to `hyperion_model_router::PrivacyTier`, the same seam that crate's own
/// doc comment already established for `ImplKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyTier {
    Local,
    ConsentedCloud,
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
    /// A publisher's own real, declared privacy tier for this implementation —
    /// `hyperion-api-gateway::router_bridge::to_router_descriptor`'s own previously-named "no
    /// per-implementation privacy tier from the Plugin Framework manifest" gap, closed for real:
    /// every bridged candidate used to be hardcoded `Local` regardless of what a publisher
    /// actually declared. `Local` (every existing manifest's default meaning, unchanged) is the
    /// conservative choice for an implementation that never leaves the device; `ConsentedCloud`
    /// is real, informed consent that this implementation may leave it.
    pub privacy_tier: PrivacyTier,
    /// docs/25 §2's `Implementation.resourceProfile` — a publisher's own real, declared resource
    /// reservation for this implementation, previously named as "not modeled — no consumer" in
    /// `hyperion-sdk`'s own doc comment. `None` (every existing manifest's default, unchanged) is
    /// honest absence, not zero cost — `hyperion-agent-runtime::AgentRuntime::prepare_invoke` (the
    /// real consumer) falls back to its own existing fixed request when a capability declares
    /// none, exactly as it always has.
    pub resource_profile: Option<ResourceVector>,
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

/// Mirrors `hyperion_device::types::DeviceType` without this crate depending on that crate (the
/// dependency runs the other way: `hyperion-device` depends on this crate for its own real
/// registration point, not the reverse — see [`HardwareSupportContribution`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareDeviceType {
    Display,
    Mobile,
    Vehicle,
    Robot,
    Wearable,
    HomeAppliance,
    Peripheral,
    Sensor,
}

/// Mirrors `hyperion_device::types::Direction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareDirection {
    Render,
    Sense,
    Actuate,
}

/// Mirrors `hyperion_device::types::SafetyClass`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HardwareSafetyClass {
    Cosmetic,
    Standard,
    High,
}

/// Mirrors `hyperion_device::types::CapabilityManifestEntry`.
#[derive(Debug, Clone, PartialEq)]
pub struct HardwareCapabilityEntry {
    pub capability_name: String,
    pub direction: HardwareDirection,
    pub safety_class: HardwareSafetyClass,
}

/// A plugin-contributed device driver profile — the real "device driver registry" entry this
/// crate's own doc comment named as missing: a plugin can teach Hyperion the expected capability
/// manifest for a known `(manufacturer, model)` pair, closing `hyperion-device`'s own "every
/// device must self-declare its full manifest with no reference to consult" gap.
/// **Never bypasses `hyperion_device::DeviceRegistry::register`'s own real signature check** —
/// docs/20 §8's device-impersonation defense still requires the device (or its driver, standing
/// in for it) to really sign over whatever manifest registration ultimately uses; this only
/// supplies what a real pairing flow can *propose* as the expected manifest instead of asking an
/// integrator to hand-write one with nothing to consult.
#[derive(Debug, Clone, PartialEq)]
pub struct HardwareSupportContribution {
    pub device_type: HardwareDeviceType,
    pub manufacturer: String,
    pub model: String,
    pub capability_manifest: Vec<HardwareCapabilityEntry>,
}

/// A plugin-contributed knowledge source: which `topic` it can supply facts about, and which
/// already-installed `Capability` (`capability_id`) actually answers a query for it. This never
/// bypasses the Capability Registry's own dispatch/consent path — it only supplies a lookup a
/// caller uses to decide *which* capability to invoke for a topic it has no local knowledge of,
/// exactly like [`HardwareSupportContribution`] only supplies what a pairing flow *proposes*,
/// never what it trusts outright.
#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeProviderContribution {
    pub topic: String,
    pub capability_id: CapabilityId,
}

/// Mirrors `hyperion_workspace::contracts::RegionAffinity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiRegionAffinity {
    Left,
    Center,
    Right,
    TopBar,
    BottomBar,
}

/// A plugin-contributed UI template for one `capability_ref` — mirrors
/// `hyperion_workspace::contracts::CapabilityUiContract`'s fields (minus per-`ComplexityTier`
/// `variants`, a real but separate Adaptive-Complexity refinement no consumer of this
/// contribution needs yet — see that crate's own doc comment) without this crate depending on
/// `hyperion-workspace` (the dependency runs the other way: `hyperion-workspace` depends on this
/// crate for its own real registration point, not the reverse).
#[derive(Debug, Clone)]
pub struct UiComponentContribution {
    pub capability_ref: String,
    pub panel_template: String,
    pub region_affinity: UiRegionAffinity,
    pub min_size: (u32, u32),
    pub priority: f32,
    pub binds_category: Option<String>,
    pub accessible_role: Option<String>,
    pub label_template: Option<String>,
    pub keyboard_operations: Vec<String>,
    pub alt_text_hook: Option<String>,
    pub contrast_ratio: f32,
    pub has_motion: bool,
    pub reduced_motion_alternative: bool,
    pub language_tag: String,
    pub emits_audio: bool,
    pub has_visual_alert_equivalent: bool,
}

/// One leaf in a plugin-contributed workflow template — mirrors `hyperion_intent`'s own
/// crate-private `TemplateLeaf` shape (that crate's own doc comment: "trimmed... to a flat,
/// non-nested HTN decomposition"). `depends_on` indexes other entries in the same
/// contribution's own `leaves`.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowLeaf {
    pub predicate: String,
    pub depends_on: Vec<usize>,
}

/// A plugin-contributed multi-step workflow template — the real registration point this
/// crate's own doc comment named `hyperion-intent`'s own `TEMPLATES` (a hardcoded, crate-private
/// static list, matched only by keyword) as missing: a plugin can now add a new named goal
/// template a real utterance can match against, alongside the built-in roster.
#[derive(Debug, Clone, PartialEq)]
pub struct AutomationWorkflowContribution {
    pub trigger_keywords: Vec<String>,
    pub root_predicate: String,
    pub leaves: Vec<WorkflowLeaf>,
}

/// Mirrors `hyperion_memory::MemoryTier` without this crate depending on that crate (the
/// dependency runs the other way: `hyperion-memory` depends on this crate for its own real
/// registration point, not the reverse — see [`MemoryProviderContribution`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryTierKind {
    Episodic,
    Semantic,
    Procedural,
    LongTerm,
}

/// A plugin-contributed external memory source: which `(tier, entity_key)` pair it can supply
/// facts about, and which already-installed `Capability` (`capability_id`) actually supplies
/// them for real — docs/24's own "memory providers register storage backends into [08 — Memory
/// Engine]" gap, closed the same honest, never-bypass-dispatch way
/// [`KnowledgeProviderContribution`] closes the analogous topic-lookup gap: this never stores or
/// retrieves a memory record itself, it only supplies a lookup a caller (`hyperion-memory`)
/// consults to decide *which* capability can supply facts about an entity it has no local record
/// of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryProviderContribution {
    pub tier: MemoryTierKind,
    pub entity_key: String,
    pub capability_id: CapabilityId,
}

/// A plugin-contributed execution engine: a real, reusable launcher program other Capability
/// implementations can run their own script/program through, instead of each one shipping a
/// whole standalone native binary — docs/24's own "execution engines register runtimes usable by
/// Capability implementations" gap. `launcher` is validated the exact same honest way a
/// `Capability`'s own `NativeBinaryDescriptor` is (`crate::registry::validate_native_binary`,
/// checked at install time, not trusted): it must really exist and really be executable.
/// `hyperion_sdk::resolve_via_engine` is the real consumer — it turns a caller's own script path
/// into a concrete `NativeBinaryDescriptor` by prepending this launcher, so a capability
/// published "via" an engine ends up installed and invoked through the exact same
/// `ImplementationKind::NativeBinary` path a hand-written native binary already uses, never a
/// second, parallel execution mechanism.
#[derive(Debug, Clone)]
pub struct ExecutionEngineContribution {
    pub engine_id: String,
    pub launcher: NativeBinaryDescriptor,
}

/// docs/24 §4's `Contribution` — every variant this crate implements now has a real owning
/// subsystem; see this crate's own doc comment for when each gained one, and for why `Model`
/// (also named in docs/24's enum) was deliberately never added as a variant here. `Agent` closes
/// docs/998-roadmap.md's own "`hyperion-coordination::catalog::default_manifests` is a
/// hardcoded, static built-in list, not a live registry a plugin's `AgentManifest` could
/// register into" gap: `crate::registry::PluginRegistry::agent_contributions` is that live
/// registry. `HardwareSupport` closes the analogous "device driver registry" gap via
/// `crate::registry::PluginRegistry::hardware_support_contributions`. `KnowledgeProvider` closes
/// `hyperion-knowledge-graph`'s own "no registry of which capability answers which topic" gap
/// via `crate::registry::PluginRegistry::knowledge_provider_contributions`. `UiComponent` closes
/// `hyperion-workspace`'s own "every `CapabilityUiContract` is hand-authored by the caller, with
/// no registry to consult" gap via `crate::registry::PluginRegistry::ui_component_contributions`.
/// `AutomationWorkflow` closes `hyperion-intent`'s own hardcoded `TEMPLATES` gap via
/// `crate::registry::PluginRegistry::automation_workflow_contributions`. `MemoryProvider` closes
/// `hyperion-memory`'s own "no external memory source registry" gap via
/// `crate::registry::PluginRegistry::memory_provider_contributions`. `ExecutionEngine` closes
/// docs/24's own "execution engines register runtimes usable by Capability implementations" gap
/// via `crate::registry::PluginRegistry::execution_engine` and `hyperion_sdk::resolve_via_engine`.
#[derive(Debug, Clone)]
pub enum Contribution {
    Capability(CapabilityManifest),
    Agent(AgentContribution),
    HardwareSupport(HardwareSupportContribution),
    KnowledgeProvider(KnowledgeProviderContribution),
    UiComponent(UiComponentContribution),
    AutomationWorkflow(AutomationWorkflowContribution),
    MemoryProvider(MemoryProviderContribution),
    ExecutionEngine(ExecutionEngineContribution),
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
    /// Carried straight over from the installing [`CapabilityManifest::privacy_tier`] --
    /// `hyperion-api-gateway::router_bridge::to_router_descriptor` is the real caller that reads
    /// this instead of hardcoding every bridged candidate as `Local`.
    pub privacy_tier: PrivacyTier,
    /// Carried straight over from the installing [`CapabilityManifest::resource_profile`] --
    /// `hyperion-agent-runtime::AgentRuntime::prepare_invoke` is the real caller that reads this
    /// as a real Scheduler admission request instead of one fixed request for every capability.
    pub resource_profile: Option<ResourceVector>,
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
    /// docs/24's own "verify against publisher's registered key" framing: a manifest whose
    /// declared `publisher` has no key registered in the real
    /// `hyperion_crypto::PublisherRegistry` a caller supplied — never silently trusted against
    /// some other key.
    #[error("no trusted key is registered for publisher {0:?}")]
    UnknownPublisher(String),
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
