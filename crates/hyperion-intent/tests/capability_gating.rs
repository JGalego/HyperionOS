//! Mirrors every other crate in this workspace: capability rights are
//! checked by the underlying `hyperion-knowledge-graph`/`hyperion-context`
//! calls this crate makes, re-checked live, never cached.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_intent::{HandleOutcome, IntentEngine, IntentError};
use hyperion_knowledge_graph::KnowledgeGraph;

#[test]
fn handle_utterance_requires_write_rights() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();

    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let engine = IntentEngine::new(graph, context);

    let result = engine.handle_utterance(
        &monitor,
        &read_only,
        "I need to launch my startup",
        "session-1",
    );
    assert!(matches!(result, Err(IntentError::Graph(_))));
}

#[test]
fn revoking_a_token_blocks_further_access_re_checked_live() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let delegate = monitor
        .cap_derive(
            &root,
            RightsMask::READ | RightsMask::WRITE,
            None,
            TrustBoundaryId(2),
        )
        .unwrap();

    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let engine = IntentEngine::new(graph, context);

    let root_id = match engine
        .handle_utterance(
            &monitor,
            &delegate,
            "I need to launch my startup",
            "session-1",
        )
        .unwrap()
    {
        HandleOutcome::Submitted(id) => id,
        other => panic!("expected Submitted, got {other:?}"),
    };
    assert!(engine.get_graph(&monitor, &delegate, root_id).is_ok());

    monitor.cap_revoke(&delegate);

    assert!(matches!(
        engine.get_graph(&monitor, &delegate, root_id),
        Err(IntentError::Graph(_))
    ));
}
