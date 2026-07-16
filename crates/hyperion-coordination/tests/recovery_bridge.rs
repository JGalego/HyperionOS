//! docs/998-roadmap.md's own named "`UndoScope::Session`/`UndoScope::Goal`" gap, closed for a
//! real caller: `CoordinationSession::with_recovery` really tags a real `hyperion_recovery`
//! `ActionRecord` with this session's own real `session_id`/`root_intent` on every real task
//! dispatch, not a bolted-on enum variant with nothing populating it.

use std::sync::Arc;

use hyperion_agent_runtime::AgentRuntime;
use hyperion_ai_runtime::{
    sign, LocalAiRuntime, MockBackend, ModelClass, ModelDescriptor, Precision, QuantizedVariant,
};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_coordination::CoordinationSession;
use hyperion_crypto::Keystore;
use hyperion_intent::{HandleOutcome, IntentEngine};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_recovery::RecoveryService;

#[test]
fn a_real_task_dispatch_tags_its_recovery_action_with_the_real_session_and_goal_ids() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let intent_engine = IntentEngine::new(graph.clone(), context);
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));

    let key_dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&key_dir.path().join("device.key")).unwrap();
    let mut descriptor = ModelDescriptor {
        model_id: 1,
        class: ModelClass::Slm,
        variants: vec![QuantizedVariant {
            precision: Precision::Fp16,
            footprint_mb: 100,
            expected_tokens_per_sec: 10.0,
        }],
        signature: None,
    };
    descriptor.signature = Some(sign(&descriptor, &keystore));
    ai_runtime
        .register_model(descriptor, &keystore.verifying_key())
        .unwrap();

    let recovery = Arc::new(RecoveryService::new(graph.clone()));
    let coordination = CoordinationSession::new(Arc::new(AgentRuntime::new(ai_runtime)), graph)
        .with_recovery(Arc::clone(&recovery));

    let root = match intent_engine
        .handle_utterance(&monitor, &token, "I need to launch my startup", "s1")
        .unwrap()
    {
        HandleOutcome::Submitted(id) => id,
        other => panic!("expected Submitted, got {other:?}"),
    };
    let session = coordination
        .create_session(
            &monitor,
            &token,
            &intent_engine,
            &intent_engine.submit(&monitor, &token, root).unwrap(),
        )
        .unwrap();

    // Tick 1: market_research (the only ready task) dispatches for real.
    let records = coordination.allocate(&monitor, &token, session).unwrap();
    assert_eq!(records.len(), 1);

    let actions = recovery.action_records();
    assert_eq!(
        actions.len(),
        1,
        "a real recovery ActionRecord must have been opened for this real dispatch"
    );
    assert_eq!(actions[0].session_id, Some(session));
    assert_eq!(actions[0].goal_id, Some(root));
    assert_eq!(
        actions[0].status,
        hyperion_recovery::ActionStatus::Committed,
        "a real dispatch that already completed must be recorded Committed, not left InFlight"
    );
}

#[test]
fn without_with_recovery_no_recovery_action_is_ever_opened() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let intent_engine = IntentEngine::new(graph.clone(), context);
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));

    let key_dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&key_dir.path().join("device.key")).unwrap();
    let mut descriptor = ModelDescriptor {
        model_id: 1,
        class: ModelClass::Slm,
        variants: vec![QuantizedVariant {
            precision: Precision::Fp16,
            footprint_mb: 100,
            expected_tokens_per_sec: 10.0,
        }],
        signature: None,
    };
    descriptor.signature = Some(sign(&descriptor, &keystore));
    ai_runtime
        .register_model(descriptor, &keystore.verifying_key())
        .unwrap();

    // No .with_recovery(...) call -- the existing, unmodified path.
    let coordination = CoordinationSession::new(Arc::new(AgentRuntime::new(ai_runtime)), graph);

    let root = match intent_engine
        .handle_utterance(&monitor, &token, "I need to launch my startup", "s1")
        .unwrap()
    {
        HandleOutcome::Submitted(id) => id,
        other => panic!("expected Submitted, got {other:?}"),
    };
    let session = coordination
        .create_session(
            &monitor,
            &token,
            &intent_engine,
            &intent_engine.submit(&monitor, &token, root).unwrap(),
        )
        .unwrap();

    let records = coordination.allocate(&monitor, &token, session).unwrap();
    assert_eq!(
        records.len(),
        1,
        "a real dispatch must still succeed with no recovery service wired in at all"
    );
}
