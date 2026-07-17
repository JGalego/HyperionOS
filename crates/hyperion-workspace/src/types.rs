use hyperion_knowledge_graph::NodeId;

use crate::contracts::{ComplexityTier, RegionAffinity};

/// docs/13 §4's `WorkspaceIntentKey` — hashes structural shape, not
/// literal content, so a second "prepare for my exam" Workspace for a
/// different subject is a cache hit on topology (§5.4).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorkspaceIntentKey {
    pub intent_shape_hash: u64,
    pub capability_set_sig: u64,
    pub complexity_tier: ComplexityTier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingMode {
    Read,
    ReadWrite,
    Stream,
}

/// docs/13 §4's `Binding`.
#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub target: NodeId,
    pub mode: BindingMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderState {
    Pending,
    Ready,
    Error,
}

/// docs/14 §4's `AccessibilityNode`, narrowed to the fields this crate's
/// linter and modality projectors actually consume.
#[derive(Debug, Clone)]
pub struct AccessibilityNode {
    pub node_id: u64,
    pub panel_ref: u64,
    pub role: String,
    pub accessible_name: String,
    pub description: String,
    pub language_tag: String,
    pub target_size: (u32, u32),
    pub is_interactive: bool,
    pub has_motion: bool,
    pub reduced_motion_alternative: bool,
    pub contrast_ratio: f32,
    pub actions: Vec<String>,
    /// docs/14 §5.7: audio-emitting nodes must carry a visual equivalent.
    pub emits_audio: bool,
    pub has_visual_alert_equivalent: bool,
}

/// docs/13 §4's `Panel`, plus its accessibility node inline rather than by
/// reference — see this crate's doc comment on why tree and layout are one
/// pass, not two.
#[derive(Debug, Clone)]
pub struct Panel {
    pub panel_id: u64,
    pub capability_ref: String,
    pub region_affinity: RegionAffinity,
    pub min_size: (u32, u32),
    pub priority: f32,
    pub bindings: Vec<Binding>,
    pub accessibility_node: AccessibilityNode,
    pub render_state: RenderState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    Generating,
    Live,
    Archived,
    Pinned,
    Discarded,
}

/// docs/13 §4's `WorkspaceGraph`.
#[derive(Debug, Clone)]
pub struct WorkspaceGraph {
    pub graph_id: u64,
    pub intent_id: NodeId,
    pub panels: Vec<Panel>,
    pub lifecycle_state: LifecycleState,
    pub created_at: u64,
}

/// docs/13 §4's `LiveUpdateEvent.event_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveUpdateEventKind {
    ResultReady,
    Progress,
    Error,
}

/// docs/13 §4's `LiveUpdateEvent` — consumed from
/// [31 — Event System](../31-event-system.md). `payload_ref` names where the
/// actual new result lives (a Knowledge Graph node), matching `Binding.target`'s
/// own `NodeId` type — the same reference a rebind writes into the affected
/// Panel's own `Binding`.
#[derive(Debug, Clone, Copy)]
pub struct LiveUpdateEvent {
    pub workspace_id: u64,
    pub panel_id: u64,
    pub event_type: LiveUpdateEventKind,
    pub payload_ref: Option<NodeId>,
}

/// docs/14 §4's `AccessibilityTree` — isomorphic to the Workspace UI
/// Graph; `focus_order` is the screen-reader/switch-scan traversal order.
#[derive(Debug, Clone)]
pub struct AccessibilityTree {
    pub tree_id: u64,
    pub workspace_graph_id: u64,
    pub nodes: Vec<AccessibilityNode>,
    pub focus_order: Vec<u64>,
}

/// docs/13 §4's `CompiledLayoutTemplate`.
#[derive(Debug, Clone)]
pub struct CompiledLayoutTemplate {
    pub template_id: u64,
    pub cache_key: WorkspaceIntentKey,
    pub panels: Vec<Panel>,
    pub accessibility_tree: AccessibilityTree,
    pub lint_result: crate::accessibility::AccessibilityLintResult,
    pub hit_count: u32,
}
