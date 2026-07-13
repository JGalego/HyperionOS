//! Mirrors every other crate in this workspace: every call is capability-
//! gated, re-checked live against the monitor.

use std::sync::Arc;

use hyperion_agent_runtime::AgentRuntime;
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_coordination::{CoordError, CoordinationSession};
use hyperion_intent::{HandleOutcome, IntentEngine};
use hyperion_knowledge_graph::KnowledgeGraph;

#[test]
fn create_session_and_allocate_require_write_rights() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root_token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root_token, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();

    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let intent_engine = IntentEngine::new(graph.clone(), context);
    let coordination = CoordinationSession::new(
        Arc::new(AgentRuntime::new(Arc::new(
            hyperion_ai_runtime::LocalAiRuntime::new(
                Box::new(hyperion_ai_runtime::MockBackend),
                8_000,
            ),
        ))),
        graph,
    );

    let root = match intent_engine
        .handle_utterance(&monitor, &root_token, "I need to launch my startup", "s1")
        .unwrap()
    {
        HandleOutcome::Submitted(id) => id,
        other => panic!("expected Submitted, got {other:?}"),
    };

    let ticket = intent_engine.submit(&monitor, &read_only, root).unwrap();
    let result = coordination.create_session(&monitor, &read_only, &intent_engine, &ticket);
    assert!(matches!(result, Err(CoordError::Unauthorized)));
}

#[test]
fn revoking_a_token_blocks_further_access_re_checked_live() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root_token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let delegate = monitor
        .cap_derive(&root_token, RightsMask::all(), None, TrustBoundaryId(2))
        .unwrap();

    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let intent_engine = IntentEngine::new(graph.clone(), context);
    let coordination = CoordinationSession::new(
        Arc::new(AgentRuntime::new(Arc::new(
            hyperion_ai_runtime::LocalAiRuntime::new(
                Box::new(hyperion_ai_runtime::MockBackend),
                8_000,
            ),
        ))),
        graph,
    );

    let root = match intent_engine
        .handle_utterance(&monitor, &delegate, "I need to launch my startup", "s1")
        .unwrap()
    {
        HandleOutcome::Submitted(id) => id,
        other => panic!("expected Submitted, got {other:?}"),
    };
    let ticket = intent_engine.submit(&monitor, &delegate, root).unwrap();
    let session = coordination
        .create_session(&monitor, &delegate, &intent_engine, &ticket)
        .unwrap();
    assert!(coordination.allocate(&monitor, &delegate, session).is_ok());

    monitor.cap_revoke(&delegate);

    assert!(matches!(
        coordination.allocate(&monitor, &delegate, session),
        Err(CoordError::Unauthorized)
    ));
}
