use hyperion_plugin_framework::{NativeBinaryDescriptor, Operation, SideEffect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustLevel {
    Sandboxed,
    Standard,
    Elevated,
}

impl TrustLevel {
    pub(crate) fn min_depth(self) -> hyperion_plugin_framework::TrustDepth {
        use hyperion_plugin_framework::TrustDepth;
        match self {
            TrustLevel::Sandboxed => TrustDepth::D0,
            TrustLevel::Standard => TrustDepth::D1,
            TrustLevel::Elevated => TrustDepth::D2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatencyClass {
    Interactive,
    Batch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    LocalModel,
    CloudApi,
    NativeBinary,
    ComposedCapability,
}

#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub operation: Operation,
    pub scope: String,
    pub justification: String,
}

/// docs/25 §2's `Contract` — the "what," ported from the doc's CDL/
/// TypeScript-flavored pseudocode into a plain Rust struct. Reuses
/// `hyperion-plugin-framework::{Operation, SideEffect}` directly rather
/// than a parallel definition.
#[derive(Debug, Clone)]
pub struct Contract {
    pub id: String,
    pub version: u32,
    pub summary: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub side_effects: Vec<SideEffect>,
    pub permissions_requested: Vec<PermissionRequest>,
    pub trust_level: TrustLevel,
}

/// docs/25 §2's `Implementation` — the "how." `resource_profile` is
/// deferred (see this crate's doc comment); `runtime: NativeBinary` is
/// the realistic mapping for an in-process Rust
/// [`crate::harness::CapabilityImplementation`] in this hosted simulator.
#[derive(Debug, Clone)]
pub struct Implementation {
    pub contract_id: String,
    pub name: String,
    pub runtime: Runtime,
    pub latency_class: LatencyClass,
    pub requires_consent: bool,
    /// Real execution info for `Runtime::NativeBinary`/`ComposedCapability` -- `None` for
    /// `LocalModel`/`CloudApi` (each already dispatches through its own real backend elsewhere).
    /// docs/998-roadmap.md's "tool creation" slice: this crate's own publish pipeline
    /// (`prepare_submission` → `publish`) now installs a `NativeBinary` submission as a genuinely
    /// *runnable* capability when this is `Some` -- naming an existing, real, already-vetted
    /// program is "tool creation" in the safe, honest sense this workspace can support today; an
    /// agent synthesizing and directly executing brand-new code from scratch is deliberately
    /// deferred (real code review/static analysis of freshly generated code before ever executing
    /// it is separate, substantial work, not a field addition).
    pub native_binary: Option<NativeBinaryDescriptor>,
}

/// docs/25 §3's `MockContextBundle` — deliberately *not* wired to the
/// real `hyperion-context` crate's richer `ContextBundle` shape; the doc
/// is explicit this is a fixture a developer hand-authors, "never live
/// data."
#[derive(Debug, Clone, Default)]
pub struct MockContextBundle {
    pub active_objects: Vec<String>,
    pub recent_intents: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct Tolerance {
    /// docs/25 §3's `tolerance.content`, a `0.0..=1.0` distance budget.
    /// `tolerance.structural` is not a field here — Layer 1's shape check
    /// (see [`crate::harness::run_harness`]) is always exact, per the
    /// doc's own "structural: exact" literal.
    pub content: f32,
}

/// docs/25 §3's `GoldenCase`.
#[derive(Debug, Clone)]
pub struct GoldenCase {
    pub case_id: String,
    pub context_bundle: MockContextBundle,
    pub input: serde_json::Value,
    pub expected_output: serde_json::Value,
    pub tolerance: Tolerance,
}

/// docs/25 §3's per-case failure taxonomy (`"structural-mismatch"`/
/// `"content-drift:{dist}"`), as a closed enum rather than a formatted
/// string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaseVerdict {
    Pass,
    StructuralMismatch,
    ContentDrift,
}

#[derive(Debug, Clone)]
pub struct ImplementationReport {
    pub implementation_name: String,
    pub verdicts: Vec<(String, CaseVerdict)>,
}

/// docs/25 §3's `runHarness` result, with the cross-implementation
/// equivalence check's findings surfaced directly rather than only
/// thrown as an exception — a caller can inspect exactly which golden
/// cases disagree across implementations.
#[derive(Debug, Clone)]
pub struct HarnessReport {
    pub per_implementation: Vec<ImplementationReport>,
    pub equivalence_violations: Vec<String>,
}

/// docs/25 §4's `PublishSubmission.reviewStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewStatus {
    AutoApproved,
    PendingHumanReview,
    Rejected,
}

/// docs/25 §4's `PublishSubmission`.
#[derive(Debug, Clone)]
pub struct PublishSubmission {
    pub package_hash: u64,
    pub contract: Contract,
    pub implementation: Implementation,
    /// Not a docs/25 field — carried here so [`crate::publish::publish`]
    /// has a real quality signal to hand
    /// `hyperion_plugin_framework::CapabilityManifest::quality_score`,
    /// which the Model Router ultimately scores implementations by. A
    /// caller derives this from [`crate::HarnessReport`]'s golden-case
    /// pass rate in a real workflow.
    pub quality_score: f32,
    pub declared_permissions: Vec<Operation>,
    pub statically_observed_permissions: Vec<Operation>,
    pub review_status: ReviewStatus,
}

#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    #[error("the implementation statically observed a permission the contract never declared requesting")]
    UndeclaredPermissionObserved,
    #[error("this submission was rejected and cannot be published")]
    SubmissionRejected,
    #[error("plugin framework error: {0}")]
    Plugin(#[from] hyperion_plugin_framework::PluginError),
}
