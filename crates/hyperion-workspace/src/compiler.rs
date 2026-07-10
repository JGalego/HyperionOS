use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_context::ContextBundle;
use hyperion_knowledge_graph::NodeId;

use crate::accessibility::{derive_node, lint_template};
use crate::contracts::{CapabilityUiContract, ComplexityTier, RegionAffinity};
use crate::types::{
    AccessibilityNode, AccessibilityTree, Binding, BindingMode, CompiledLayoutTemplate,
    LifecycleState, Panel, RenderState, WorkspaceGraph, WorkspaceIntentKey,
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
}

/// docs/13 — Dynamic UI Runtime's Workspace Compiler, fused with docs/14's
/// accessibility tree derivation. See this crate's doc comment for what's
/// deferred.
pub struct WorkspaceCompiler {
    template_cache: Mutex<HashMap<WorkspaceIntentKey, CompiledLayoutTemplate>>,
    graphs: Mutex<HashMap<u64, WorkspaceGraph>>,
    next_id: AtomicU64,
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
        }
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
    pub fn get_template(
        &self,
        intent_predicate: &str,
        contracts: &[CapabilityUiContract],
        tier: ComplexityTier,
    ) -> Option<CompiledLayoutTemplate> {
        let key = Self::cache_key(intent_predicate, contracts, tier);
        self.template_cache.lock().unwrap().get(&key).cloned()
    }
}
