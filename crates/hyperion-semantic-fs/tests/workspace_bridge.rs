//! This phase's "workspace generation" mandate item: a VirtualFolder
//! becomes a real hyperion-workspace WorkspaceGraph.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_semantic_fs::{present_as_workspace, QuerySpec, SemanticFilesystem};
use hyperion_workspace::WorkspaceCompiler;
use serde_json::json;

#[test]
fn a_search_result_folder_compiles_into_a_workspace_with_one_bound_panel() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let fs = SemanticFilesystem::new(graph.clone(), context);
    let compiler = WorkspaceCompiler::new();

    let trip = graph
        .put_node(
            &monitor,
            &token,
            None,
            "trip",
            None,
            json!({"title": "Hawaii"}),
        )
        .unwrap();
    let photo = graph
        .put_node(
            &monitor,
            &token,
            None,
            "photo",
            None,
            json!({"title": "beach"}),
        )
        .unwrap();
    graph
        .link(
            &monitor,
            &token,
            photo,
            "part_of_trip",
            trip,
            1.0,
            hyperion_knowledge_graph::EdgeOrigin::Inferred,
            Some(0.9),
            "agent",
            None,
        )
        .unwrap();

    let folder = fs
        .query(
            &monitor,
            &token,
            &QuerySpec {
                anchor: Some(trip),
                hop_bound: 1,
                ..Default::default()
            },
        )
        .unwrap();

    let workspace = present_as_workspace(&compiler, &monitor, &token, &folder, trip).unwrap();
    assert_eq!(workspace.panels.len(), 1);
    assert_eq!(
        workspace.panels[0].bindings.len(),
        folder.member_object_ids.len()
    );
    assert!(workspace.panels[0]
        .bindings
        .iter()
        .any(|b| b.target == photo));
}
