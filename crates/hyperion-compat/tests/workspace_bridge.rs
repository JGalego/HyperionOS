//! docs/27's "Window-to-Workspace binding" and "Accessibility bridging
//! (bounded exception)": present_as_workspace wraps a real Compatibility
//! session as a real hyperion-workspace WorkspaceGraph, binding its
//! promoted artifacts and carrying the docs/27 disclosure for real.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_compat::{
    present_as_workspace, AccessibilityBridgeTier, CompatHost, CompatibilityProfile, LegacyTarget,
    NetworkPolicy, PromotionPolicy, SessionId, TrustDepth,
};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_netstack::{MockExtractionBackend, MockFetchBackend, NetstackHub};
use hyperion_workspace::WorkspaceCompiler;

fn setup(
    tier: AccessibilityBridgeTier,
) -> (
    CapabilityMonitor,
    CapabilityToken,
    CompatHost,
    WorkspaceCompiler,
    SessionId,
) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let netstack = Arc::new(NetstackHub::new(
        graph.clone(),
        Box::new(MockFetchBackend::new()),
        Box::new(MockExtractionBackend),
    ));
    let host = CompatHost::new(graph, netstack);
    let compiler = WorkspaceCompiler::new();

    let profile = CompatibilityProfile {
        target: LegacyTarget::Linux,
        min_depth: TrustDepth::D1,
        network_default: NetworkPolicy::Deny,
        filesystem_roots: vec!["/home/guest/Documents".to_string()],
        accessibility_bridge: tier,
    };
    let session = host
        .launch(&mut monitor, &root, profile, TrustDepth::D2, 1_000)
        .unwrap();

    (monitor, root, host, compiler, session)
}

#[test]
fn a_platform_bridged_session_gets_a_normal_looking_accessible_name() {
    let (monitor, root, host, compiler, session) = setup(AccessibilityBridgeTier::Platform);
    let intent_id = hyperion_storage::ObjectId(1);

    let graph =
        present_as_workspace(&host, &compiler, &monitor, &root, session, intent_id, 1_000).unwrap();

    assert_eq!(graph.intent_id, intent_id);
    assert_eq!(graph.panels.len(), 1);
    assert_eq!(
        graph.panels[0].accessibility_node.accessible_name, "Linux application",
        "a real platform bridge needs no disclosure"
    );
}

#[test]
fn a_pixel_fallback_session_carries_the_docs27_disclosure() {
    let (monitor, root, host, compiler, session) = setup(AccessibilityBridgeTier::PixelFallback);
    let intent_id = hyperion_storage::ObjectId(1);

    let graph =
        present_as_workspace(&host, &compiler, &monitor, &root, session, intent_id, 1_000).unwrap();

    assert_eq!(
        graph.panels[0].accessibility_node.accessible_name,
        "Limited accessibility: legacy application"
    );
}

#[test]
fn a_no_bridge_session_also_carries_the_disclosure() {
    let (monitor, root, host, compiler, session) = setup(AccessibilityBridgeTier::None);
    let intent_id = hyperion_storage::ObjectId(1);

    let graph =
        present_as_workspace(&host, &compiler, &monitor, &root, session, intent_id, 1_000).unwrap();

    assert_eq!(
        graph.panels[0].accessibility_node.accessible_name,
        "Limited accessibility: legacy application",
        "PixelFallback and None both surface the same disclosure -- neither claims Platform parity"
    );
}

#[test]
fn the_disclosure_node_itself_passes_the_real_accessibility_linter() {
    let (monitor, root, host, compiler, session) = setup(AccessibilityBridgeTier::None);
    let intent_id = hyperion_storage::ObjectId(1);

    let graph =
        present_as_workspace(&host, &compiler, &monitor, &root, session, intent_id, 1_000).unwrap();

    let tree = hyperion_workspace::AccessibilityTree {
        tree_id: 0,
        workspace_graph_id: graph.graph_id,
        nodes: vec![graph.panels[0].accessibility_node.clone()],
        focus_order: vec![],
    };
    let lint = hyperion_workspace::lint_template(&tree, 1.0);
    assert!(
        lint.passed,
        "the disclosure node must itself be a real, valid accessibility node: {:?}",
        lint.violations
    );
}

#[test]
fn a_promoted_artifact_is_bound_to_the_panel_but_a_merely_captured_one_is_not() {
    let (mut monitor, root, host, compiler, session) = setup(AccessibilityBridgeTier::Platform);
    host.grant(&mut monitor, &root, session, RightsMask::WRITE)
        .unwrap();

    // Two files: one promoted (Stage B), one only ever captured (Stage A).
    host.shim_open(session, "/home/guest/Documents/report.txt", true)
        .unwrap();
    host.shim_open(session, "/home/guest/Documents/draft.txt", true)
        .unwrap();
    let promoted_id = host
        .promote_artifact(
            &monitor,
            &root,
            session,
            "/home/guest/Documents/report.txt",
            PromotionPolicy::AskEveryTime,
            "Document",
            serde_json::json!({"title": "Quarterly Report"}),
            true,
        )
        .unwrap();

    let intent_id = hyperion_storage::ObjectId(1);
    let graph =
        present_as_workspace(&host, &compiler, &monitor, &root, session, intent_id, 1_000).unwrap();

    let bound_targets: Vec<_> = graph.panels[0].bindings.iter().map(|b| b.target).collect();
    assert_eq!(
        bound_targets,
        vec![promoted_id],
        "only the promoted artifact is bound -- a captured-but-not-promoted one never reaches the panel"
    );
}

#[test]
fn a_session_with_nothing_promoted_yet_compiles_with_no_bindings() {
    let (monitor, root, host, compiler, session) = setup(AccessibilityBridgeTier::Platform);
    let intent_id = hyperion_storage::ObjectId(1);

    let graph =
        present_as_workspace(&host, &compiler, &monitor, &root, session, intent_id, 1_000).unwrap();

    assert!(graph.panels[0].bindings.is_empty());
}
