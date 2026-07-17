use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_context::ContextBundle;
use hyperion_events::{
    BackpressurePolicy, DeliveryClass, EventBus, EventPayload, SubjectId, Subscription, Topic,
    TopicKind, TopicPattern,
};
use hyperion_knowledge_graph::NodeId;

use crate::accessibility::{derive_node, lint_template};
use crate::contracts::{CapabilityUiContract, ComplexityTier, RegionAffinity};
use crate::types::{
    AccessibilityNode, AccessibilityTree, Binding, BindingMode, CompiledLayoutTemplate,
    LifecycleState, LiveUpdateEvent, LiveUpdateEventKind, Panel, RenderState, WorkspaceGraph,
    WorkspaceIntentKey,
};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs()
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("no such workspace graph")]
    NotFound,
    #[error("invalid lifecycle transition: {0}")]
    InvalidTransition(String),
    #[error("no such panel in this workspace graph")]
    NoSuchPanel,
    #[error("this compiler has no Event System bus wired (see WorkspaceCompiler::with_events)")]
    NoEventBus,
    #[error("event bus rejected the request: {0}")]
    EventBus(#[from] hyperion_events::EventFault),
}

impl LiveUpdateEvent {
    /// Reconstructs a domain [`LiveUpdateEvent`] from the raw wire event a real
    /// `hyperion_events::Subscription::recv`/`try_recv` yields — the reverse of what
    /// [`WorkspaceCompiler::publish_live_update`] encodes. `None` if `payload` isn't the shape
    /// this crate itself publishes — a `Subtree` subscription is, in general, capable of matching
    /// any topic under its root, not only ones this crate's own publisher produced.
    pub fn from_payload(payload: &EventPayload) -> Option<Self> {
        let EventPayload::Inline(value) = payload else {
            return None;
        };
        Some(LiveUpdateEvent {
            workspace_id: value["workspace_id"].as_u64()?,
            panel_id: value["panel_id"].as_u64()?,
            event_type: match value["event_type"].as_str()? {
                "result_ready" => LiveUpdateEventKind::ResultReady,
                "progress" => LiveUpdateEventKind::Progress,
                "error" => LiveUpdateEventKind::Error,
                _ => return None,
            },
            payload_ref: value["payload_ref"]
                .as_u64()
                .map(hyperion_storage::ObjectId),
        })
    }
}

/// docs/13 — Dynamic UI Runtime's Workspace Compiler, fused with docs/14's
/// accessibility tree derivation. See this crate's doc comment for what's
/// deferred.
pub struct WorkspaceCompiler {
    template_cache: Mutex<HashMap<WorkspaceIntentKey, CompiledLayoutTemplate>>,
    graphs: Mutex<HashMap<u64, WorkspaceGraph>>,
    next_id: AtomicU64,
    /// docs/13 §5.5's own named "live Workspace subscribes to the Event System" gap: real,
    /// optional (the same `Option<Arc<...>>` shape this workspace uses for every other optional
    /// backend) once [`Self::with_events`] wires it. See [`Self::publish_live_update`]/
    /// [`Self::subscribe_live`]/[`Self::apply_live_update`].
    events: Option<Arc<EventBus>>,
}

impl Default for WorkspaceCompiler {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceCompiler {
    pub fn new() -> Self {
        WorkspaceCompiler {
            template_cache: Mutex::new(HashMap::new()),
            graphs: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            events: None,
        }
    }

    /// Opts this compiler into docs/31-event-system.md's real broadcast bus, closing this
    /// crate's own previously-named "this crate has no `LiveUpdateEvent` subscription; a caller
    /// can re-derive/re-bind by calling compile again, which is correct but not incremental" gap.
    pub fn with_events(mut self, events: Arc<EventBus>) -> Self {
        self.events = Some(events);
        self
    }

    fn require(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        rights: RightsMask,
    ) -> Result<(), WorkspaceError> {
        monitor
            .check_rights_ok_result(token, rights)
            .map_err(|_| WorkspaceError::Unauthorized)
    }

    fn cache_key(
        intent_predicate: &str,
        contracts: &[CapabilityUiContract],
        tier: ComplexityTier,
    ) -> WorkspaceIntentKey {
        let mut capability_refs: Vec<&str> = contracts
            .iter()
            .map(|c| c.capability_ref.as_str())
            .collect();
        capability_refs.sort_unstable();
        WorkspaceIntentKey {
            intent_shape_hash: fnv1a64(intent_predicate.as_bytes()),
            capability_set_sig: fnv1a64(capability_refs.join(",").as_bytes()),
            complexity_tier: tier,
        }
    }

    /// Recomputes a panel's accessibility node from `contracts` in place. The template cache
    /// key above is deliberately coarse (predicate + capability-set + tier, not full contract
    /// content) so two turns with the same *shape* -- e.g. two different "generic_goal"
    /// responses -- reuse the same real layout decisions (panel count/size/position, lint
    /// result) instead of redoing that work every time. But `build_template` also bakes each
    /// contract's own content (`label_template`, surfaced as `accessible_name`) into the very
    /// same cached `Panel`/`AccessibilityNode` -- a cache hit must never let a *later* call
    /// silently redisplay an *earlier* call's content just because the shape matched. Called on
    /// every cache hit *and* miss (a fresh build's own panels already carry today's content, but
    /// re-deriving is cheap and keeps both paths honest rather than trusting `build_template`'s
    /// own freshness by convention).
    fn refresh_panel_node(panel: &mut Panel, contracts: &[CapabilityUiContract]) {
        if let Some(contract) = contracts
            .iter()
            .find(|c| c.capability_ref == panel.capability_ref)
        {
            panel.accessibility_node = derive_node(
                panel.accessibility_node.node_id,
                panel.panel_id,
                contract,
                !panel.bindings.is_empty(),
            );
        }
    }

    /// docs/13 §7's `compile_workspace` + docs/14 §5.1's tree derivation,
    /// fused. `target_size_multiplier` stands in for a caller-supplied
    /// `UserAccessibilityProfile.target_size_multiplier` (docs/14 §4) —
    /// this crate has no real profile store yet.
    #[allow(clippy::too_many_arguments)]
    pub fn compile(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        intent_id: NodeId,
        intent_predicate: &str,
        contracts: &[CapabilityUiContract],
        context_bundle: &ContextBundle,
        tier: ComplexityTier,
        target_size_multiplier: f32,
    ) -> Result<WorkspaceGraph, WorkspaceError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let key = Self::cache_key(intent_predicate, contracts, tier);
        let mut cache = self.template_cache.lock().unwrap();
        let template = match cache.get(&key) {
            Some(t) if t.lint_result.passed => t.clone(),
            _ => {
                let template =
                    self.build_template(key.clone(), contracts, tier, target_size_multiplier);
                cache.insert(key.clone(), template.clone());
                template
            }
        };
        if let Some(cached) = cache.get_mut(&key) {
            cached.hit_count += 1;
        }
        drop(cache);

        // docs/13 §5.1: bind each Panel to the Context Bundle entries its
        // Capability declared relevance for.
        let mut panels = template.panels.clone();
        for panel in &mut panels {
            let category = contracts
                .iter()
                .find(|c| c.capability_ref == panel.capability_ref)
                .and_then(|c| c.binds_category.as_deref());
            if let Some(category) = category {
                panel.bindings = context_bundle
                    .entries
                    .iter()
                    .filter(|e| e.category == category)
                    .map(|e| Binding {
                        target: e.node_id,
                        mode: BindingMode::Read,
                    })
                    .collect();
            }
            panel.render_state = if panel.bindings.is_empty() {
                RenderState::Pending
            } else {
                RenderState::Ready
            };
            Self::refresh_panel_node(panel, contracts);
        }

        let graph_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let graph = WorkspaceGraph {
            graph_id,
            intent_id,
            panels,
            lifecycle_state: LifecycleState::Generating,
            created_at: now(),
        };
        self.graphs.lock().unwrap().insert(graph_id, graph.clone());
        Ok(graph)
    }

    fn build_template(
        &self,
        key: WorkspaceIntentKey,
        contracts: &[CapabilityUiContract],
        tier: ComplexityTier,
        target_size_multiplier: f32,
    ) -> CompiledLayoutTemplate {
        let mut panels = Vec::with_capacity(contracts.len());
        let mut nodes = Vec::with_capacity(contracts.len());
        let mut focus_order = Vec::new();

        for (i, contract) in contracts.iter().enumerate() {
            let panel_id = i as u64 + 1;
            let (_, min_size) = contract.resolve_variant(tier);
            let node = derive_node(panel_id, panel_id, contract, false);
            if node.is_interactive {
                focus_order.push(node.node_id);
            }
            nodes.push(node.clone());
            panels.push(Panel {
                panel_id,
                capability_ref: contract.capability_ref.clone(),
                region_affinity: contract.region_affinity,
                min_size,
                priority: contract.priority,
                bindings: Vec::new(),
                accessibility_node: node,
                render_state: RenderState::Pending,
            });
        }

        let tree_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let tree = AccessibilityTree {
            tree_id,
            workspace_graph_id: 0,
            nodes,
            focus_order,
        };
        let lint_result = lint_template(&tree, target_size_multiplier);

        let (panels, tree, lint_result) = if lint_result.passed {
            (panels, tree, lint_result)
        } else {
            let (fallback_panels, fallback_tree) = Self::fallback_template();
            let fallback_lint = lint_template(&fallback_tree, target_size_multiplier);
            (fallback_panels, fallback_tree, fallback_lint)
        };

        CompiledLayoutTemplate {
            template_id: self.next_id.fetch_add(1, Ordering::Relaxed),
            cache_key: key,
            panels,
            accessibility_tree: tree,
            lint_result,
            hit_count: 0,
        }
    }

    /// docs/13 §7's `fallback_generic_template` / docs/14 §7's
    /// `generate_fallback_template`: a single guaranteed-valid "raw data
    /// viewer" panel, never a cyclic or inaccessible graph — Design
    /// Invariant 5, "degrade, never fail closed."
    fn fallback_template() -> (Vec<Panel>, AccessibilityTree) {
        let node = AccessibilityNode {
            node_id: 1,
            panel_ref: 1,
            role: "generic".to_string(),
            accessible_name: "Raw data viewer".to_string(),
            description: "A generic fallback view for content that could not be laid out normally"
                .to_string(),
            language_tag: "en".to_string(),
            target_size: (64, 64),
            is_interactive: true,
            has_motion: false,
            reduced_motion_alternative: true,
            contrast_ratio: 7.0,
            actions: vec!["view".to_string()],
            emits_audio: false,
            has_visual_alert_equivalent: true,
        };
        let panel = Panel {
            panel_id: 1,
            capability_ref: "generic.raw_data_viewer".to_string(),
            region_affinity: RegionAffinity::Center,
            min_size: (200, 200),
            priority: 0.0,
            bindings: Vec::new(),
            accessibility_node: node.clone(),
            render_state: RenderState::Pending,
        };
        let tree = AccessibilityTree {
            tree_id: 0,
            workspace_graph_id: 0,
            nodes: vec![node],
            focus_order: vec![1],
        };
        (vec![panel], tree)
    }

    /// `WorkspaceRenderer.mount` — docs/13 §6. No real paint; advances the
    /// lifecycle state only, per this crate's doc comment.
    pub fn mount(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        graph_id: u64,
    ) -> Result<(), WorkspaceError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let mut graphs = self.graphs.lock().unwrap();
        let graph = graphs.get_mut(&graph_id).ok_or(WorkspaceError::NotFound)?;
        if graph.lifecycle_state != LifecycleState::Generating {
            return Err(WorkspaceError::InvalidTransition(format!(
                "cannot mount from {:?}",
                graph.lifecycle_state
            )));
        }
        graph.lifecycle_state = LifecycleState::Live;
        Ok(())
    }

    /// docs/13 §6's `EventBus.subscribe(workspace_id, handler)`: a live Workspace's own
    /// subscription to every `LiveUpdateEvent` published against any of its Panels.
    /// `TopicPattern::Subtree` (rather than one subscription per Panel) matches docs/13 §5.5's
    /// "the live Workspace subscribes" — one subscription per Workspace, not per Panel — and
    /// `Coalesce` backpressure now keeps one independent slot per Panel's own topic (see
    /// `hyperion-events`'s own fix for why this is safe: a rapid update to Panel A no longer
    /// clobbers a still-pending update to Panel B).
    pub fn subscribe_live(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        graph_id: u64,
    ) -> Result<Subscription, WorkspaceError> {
        self.require(monitor, token, RightsMask::READ)?;
        let bus = self.events.as_ref().ok_or(WorkspaceError::NoEventBus)?;
        Ok(bus.subscribe(
            monitor,
            token,
            token.origin(),
            TopicPattern::Subtree {
                kind: TopicKind::WorkspaceTrigger,
                root: SubjectId::Object(graph_id),
            },
            DeliveryClass::AtMostOnce,
            BackpressurePolicy::Coalesce,
        )?)
    }

    /// The publish half of docs/13 §5.5: an Agent (or any real result producer) announces that
    /// one Panel changed. A no-op if this compiler has no bus wired — publishing is additive,
    /// never a precondition for the Workspace itself functioning (the same real-vs-optional shape
    /// `hyperion-netstack::NetstackHub::publish_entity_resolved` already established).
    pub fn publish_live_update(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        event: &LiveUpdateEvent,
    ) -> Result<(), WorkspaceError> {
        let Some(bus) = &self.events else {
            return Ok(());
        };
        let topic = Topic::new(
            TopicKind::WorkspaceTrigger,
            SubjectId::Object(event.panel_id),
            "workspace.live_update.v1",
        );
        let event_type = match event.event_type {
            LiveUpdateEventKind::ResultReady => "result_ready",
            LiveUpdateEventKind::Progress => "progress",
            LiveUpdateEventKind::Error => "error",
        };
        let payload = EventPayload::Inline(serde_json::json!({
            "workspace_id": event.workspace_id,
            "panel_id": event.panel_id,
            "event_type": event_type,
            "payload_ref": event.payload_ref.map(|n| n.0),
        }));
        bus.publish(
            monitor,
            token,
            token.origin(),
            topic,
            payload,
            vec![event.workspace_id],
        )?;
        Ok(())
    }

    /// docs/13 §5.5: "only the affected Panel's binding is diffed and patched — never the whole
    /// graph." Mutates exactly the one `Panel` named by `event.panel_id`'s `render_state` (and,
    /// when the event carries a `payload_ref`, its first `Binding`'s `target`) — every other
    /// Panel in `graph_id`'s `WorkspaceGraph` is untouched, unlike a full `compile()` re-run.
    pub fn apply_live_update(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        graph_id: u64,
        event: &LiveUpdateEvent,
    ) -> Result<(), WorkspaceError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let mut graphs = self.graphs.lock().unwrap();
        let graph = graphs.get_mut(&graph_id).ok_or(WorkspaceError::NotFound)?;
        let panel = graph
            .panels
            .iter_mut()
            .find(|p| p.panel_id == event.panel_id)
            .ok_or(WorkspaceError::NoSuchPanel)?;
        panel.render_state = match event.event_type {
            LiveUpdateEventKind::ResultReady => RenderState::Ready,
            LiveUpdateEventKind::Progress => RenderState::Pending,
            LiveUpdateEventKind::Error => RenderState::Error,
        };
        if let Some(target) = event.payload_ref {
            // `compile()` never itself populates `bindings` (this crate's own scope: a Binding
            // is created once real Semantic Object content exists to bind to, which a live
            // result genuinely is the first instance of for many Panels) -- so a Panel with no
            // existing binding gets a new one rather than having nothing to patch; a Panel that
            // already has one is patched in place, never duplicated.
            match panel.bindings.first_mut() {
                Some(binding) => binding.target = target,
                None => panel.bindings.push(Binding {
                    target,
                    mode: BindingMode::Read,
                }),
            }
        }
        Ok(())
    }

    /// `Workspace.pin` — docs/13 §6: promotes into a durable record. This
    /// crate returns the graph id itself as the stand-in "durable Semantic
    /// Object id" (no real Knowledge Graph write here — the graph's bound
    /// Semantic Objects are already durable; only the graph shell would
    /// need writing, which a real integration wires into
    /// `hyperion-knowledge-graph`).
    pub fn pin(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        graph_id: u64,
    ) -> Result<u64, WorkspaceError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let mut graphs = self.graphs.lock().unwrap();
        let graph = graphs.get_mut(&graph_id).ok_or(WorkspaceError::NotFound)?;
        if graph.lifecycle_state == LifecycleState::Discarded {
            return Err(WorkspaceError::InvalidTransition(
                "cannot pin a discarded workspace".to_string(),
            ));
        }
        graph.lifecycle_state = LifecycleState::Pinned;
        Ok(graph_id)
    }

    pub fn archive(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        graph_id: u64,
    ) -> Result<u64, WorkspaceError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let mut graphs = self.graphs.lock().unwrap();
        let graph = graphs.get_mut(&graph_id).ok_or(WorkspaceError::NotFound)?;
        if graph.lifecycle_state == LifecycleState::Discarded {
            return Err(WorkspaceError::InvalidTransition(
                "cannot archive a discarded workspace".to_string(),
            ));
        }
        graph.lifecycle_state = LifecycleState::Archived;
        Ok(graph_id)
    }

    /// docs/13 §5.6: pinning is the escape hatch from ephemerality — a
    /// pinned Workspace is never silently discarded.
    pub fn discard(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        graph_id: u64,
    ) -> Result<(), WorkspaceError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let mut graphs = self.graphs.lock().unwrap();
        let graph = graphs.get_mut(&graph_id).ok_or(WorkspaceError::NotFound)?;
        if graph.lifecycle_state == LifecycleState::Pinned {
            return Err(WorkspaceError::InvalidTransition(
                "cannot discard a pinned workspace".to_string(),
            ));
        }
        graph.lifecycle_state = LifecycleState::Discarded;
        Ok(())
    }

    pub fn get_graph(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        graph_id: u64,
    ) -> Result<WorkspaceGraph, WorkspaceError> {
        self.require(monitor, token, RightsMask::READ)?;
        self.graphs
            .lock()
            .unwrap()
            .get(&graph_id)
            .cloned()
            .ok_or(WorkspaceError::NotFound)
    }

    /// Exposed for tests/callers that want to observe cache behavior
    /// directly (hit_count, lint_result) — docs/13 §5.4's cache reuse is
    /// otherwise an internal, invisible optimization.
    ///
    /// Also the one real caller this crate has today (`hyperion_console::ConsoleSession`) uses
    /// this, not [`Self::compile`]'s own returned `WorkspaceGraph`, to get the
    /// `AccessibilityTree` it actually projects and displays -- so this refreshes each panel's
    /// (and the parallel tree node's) content from `contracts` exactly like `compile` does,
    /// same reasoning as [`Self::refresh_panel_node`]'s own doc comment: a cache hit must never
    /// silently redisplay an earlier call's content just because the shape matched.
    pub fn get_template(
        &self,
        intent_predicate: &str,
        contracts: &[CapabilityUiContract],
        tier: ComplexityTier,
    ) -> Option<CompiledLayoutTemplate> {
        let key = Self::cache_key(intent_predicate, contracts, tier);
        let mut template = self.template_cache.lock().unwrap().get(&key).cloned()?;
        for panel in &mut template.panels {
            Self::refresh_panel_node(panel, contracts);
        }
        // `accessibility_tree.nodes` and `panels` are built from the same per-contract loop in
        // `build_template`, in the same order and with the same node_id == panel_id, so mirroring
        // each now-refreshed panel's node onto its matching tree node keeps both in lockstep.
        for node in &mut template.accessibility_tree.nodes {
            if let Some(panel) = template
                .panels
                .iter()
                .find(|p| p.accessibility_node.node_id == node.node_id)
            {
                *node = panel.accessibility_node.clone();
            }
        }
        Some(template)
    }
}
