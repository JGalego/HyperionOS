use hyperion_capability::{CapabilityToken, TrustBoundaryId};
use hyperion_knowledge_graph::NodeId;
use hyperion_netstack::FetchedPage;

pub type SessionId = u64;

/// docs/27 §1's `LegacyTarget`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyTarget {
    Windows,
    Linux,
    Android,
    Web,
    Cli,
    Vm,
    Container,
}

/// docs/03's sandboxing depth spectrum, duplicated from
/// `hyperion-plugin-framework::TrustDepth` rather than shared — both
/// crates use it as a policy label over the *same* underlying
/// `hyperion-capability::TrustBoundaryId` primitive, but reusing a
/// trivial four-value enum across two otherwise-unrelated domains
/// (Plugins vs. legacy-app compatibility sessions) isn't worth the
/// dependency edge it would create.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustDepth {
    D0,
    D1,
    D2,
    D3,
}

impl LegacyTarget {
    /// docs/27 §1's per-target default depth table.
    pub fn default_depth(self) -> TrustDepth {
        match self {
            LegacyTarget::Windows | LegacyTarget::Vm => TrustDepth::D3,
            LegacyTarget::Android | LegacyTarget::Container => TrustDepth::D2,
            LegacyTarget::Linux | LegacyTarget::Cli => TrustDepth::D1,
            LegacyTarget::Web => TrustDepth::D0,
        }
    }
}

/// docs/27 §5's `NetworkPolicy`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkPolicy {
    Deny,
    LoopbackOnly,
    Allow { scope: String },
}

/// docs/27's "Accessibility bridging (bounded exception to
/// [02 §4](../02-core-architecture.md#4-design-invariants)'s Invariant 6)"
/// tiers: opaque pixel content has no Capability contract for [14 —
/// Accessibility](../14-accessibility.md)'s compiler pass to derive a real
/// tree from, so the Compatibility Host declares which mitigation is
/// active instead of silently claiming full accessibility. `Platform`
/// (a real platform accessibility-API bridge) is the only tier docs/27
/// treats as not needing a user-facing disclosure; `PixelFallback` and
/// `None` both surface [`crate::workspace_bridge::present_as_workspace`]'s
/// "Limited accessibility: legacy application" notice — see that
/// function's doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessibilityBridgeTier {
    Platform,
    PixelFallback,
    None,
}

/// docs/27 §2's `CompatibilityProfile`, `filesystem_roots` narrowed from
/// `Vec<SemanticRoot>` to plain guest-path-prefix strings —
/// `ShimPathMapping`'s `semantic_root` linkage is deferred (see this
/// crate's doc comment).
#[derive(Debug, Clone)]
pub struct CompatibilityProfile {
    pub target: LegacyTarget,
    pub min_depth: TrustDepth,
    pub network_default: NetworkPolicy,
    pub filesystem_roots: Vec<String>,
    pub accessibility_bridge: AccessibilityBridgeTier,
}

/// docs/27 §2's `CompatSession`. "The guest itself holds zero capability
/// tokens — every token belongs to the Compatibility Host mediating on
/// its behalf" (confused-deputy prevention, same invariant as docs/03).
#[derive(Debug, Clone)]
pub struct CompatSession {
    pub session_id: SessionId,
    pub boundary: TrustBoundaryId,
    pub profile: CompatibilityProfile,
    pub grants: Vec<CapabilityToken>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromotionState {
    Pending,
    Promoted,
    Ignored,
}

/// docs/27 §2's `IngestedArtifact`, `draft_object` renamed
/// `draft_metadata` (a `serde_json::Value`, this workspace's usual
/// Semantic Object metadata shape, rather than a separate
/// `SemanticObjectDraft` type).
#[derive(Debug, Clone)]
pub struct IngestedArtifact {
    pub guest_path: String,
    pub sniffed_type: String,
    pub promotion_state: PromotionState,
    pub draft_metadata: Option<serde_json::Value>,
    pub promoted_object_id: Option<NodeId>,
}

/// docs/27 §3's promotion consent gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromotionPolicy {
    AskEveryTime,
    StandingRuleApprove,
    StandingRuleDeny,
}

/// The real result of one [`crate::host::CompatHost::exec_in_sandbox`] call -- a genuine child
/// process, run to completion under real Linux namespace isolation (see that method's own doc
/// comment), not a simulated exit code.
#[derive(Debug, Clone)]
pub struct SandboxExecution {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// The real result of one [`crate::host::CompatHost::render_web_page`] call: `fetched` is the
/// same [`FetchedPage`] `web_fetch` already returns (the mediated, audited, rate-limited half),
/// `rendered_dom` is the real post-load DOM a headless browser engine produced for the identical
/// URL -- see that method's own doc comment for why these are two honestly-separate real results
/// rather than one.
#[derive(Debug, Clone)]
pub struct RenderedPage {
    pub fetched: FetchedPage,
    pub rendered_dom: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CompatError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("guest_path does not match any declared filesystem root")]
    PathOutsideDeclaredRoots,
    #[error("this session has no write grant for its declared roots")]
    WriteNotGranted,
    #[error("promotion was declined")]
    PromotionDeclined,
    #[error("no such compatibility session")]
    NoSuchSession,
    #[error("no such captured artifact")]
    NoSuchArtifact,
    #[error("this operation is only valid for a Web-target session with network access allowed")]
    NotAnAllowedWebSession,
    #[error("this operation is only valid for a Linux/Container/Cli-target session")]
    NotASandboxableSession,
    #[error("no real namespace-sandboxing tool (bwrap) is available on this host")]
    SandboxUnavailable,
    #[error("no real headless browser engine is available on this host")]
    BrowserUnavailable,
    #[error("sandboxed process could not be spawned: {0}")]
    SandboxSpawnFailed(String),
    #[error("headless render failed: {0}")]
    RenderFailed(String),
    #[error("capability fault: {0}")]
    Capability(#[from] hyperion_capability::Fault),
    #[error("knowledge graph error: {0}")]
    Graph(#[from] hyperion_knowledge_graph::GraphError),
    #[error("netstack error: {0}")]
    Netstack(#[from] hyperion_netstack::NetstackError),
    #[error("workspace error: {0}")]
    Workspace(#[from] hyperion_workspace::WorkspaceError),
}
