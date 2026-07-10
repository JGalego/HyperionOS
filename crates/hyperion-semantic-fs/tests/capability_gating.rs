//! Mirrors every other crate in this workspace: every call is capability-
//! gated, re-checked live against the monitor.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_semantic_fs::{FsError, QuerySpec, SemanticFilesystem};
use serde_json::json;

#[test]
fn mkcollection_requires_write_rights() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();

    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let fs = SemanticFilesystem::new(graph, context);

    let result = fs.mkcollection(&monitor, &read_only, "Receipts", None);
    assert!(matches!(result, Err(FsError::Unauthorized)));
}

#[test]
fn revoking_a_token_blocks_further_access_re_checked_live() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let delegate = monitor
        .cap_derive(&root, RightsMask::all(), None, TrustBoundaryId(2))
        .unwrap();

    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let fs = SemanticFilesystem::new(graph.clone(), context);

    let trip = graph
        .put_node(
            &monitor,
            &delegate,
            None,
            "trip",
            None,
            json!({"title": "Hawaii"}),
        )
        .unwrap();
    let spec = QuerySpec {
        anchor: Some(trip),
        hop_bound: 1,
        ..Default::default()
    };
    assert!(fs.query(&monitor, &delegate, &spec).is_ok());

    monitor.cap_revoke(&delegate);

    assert!(matches!(
        fs.query(&monitor, &delegate, &spec),
        Err(FsError::Unauthorized)
    ));
}
