//! docs/12 §12's "object-affinity" plan partitioning, real end to end: `partition_version` starts
//! at 0 and really increments as real task-status changes land through a live `allocate` pass --
//! not just the pure `task_partition_key` unit tests in `src/engine.rs`, which this workspace's
//! one built-in HTN template (a single connected chain) can't exercise on its own (there's only
//! ever one real partition to observe here).

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

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    IntentEngine,
    CoordinationSession,
) {
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
        .expect("a descriptor this test just signed always verifies");

    let coordination = CoordinationSession::new(Arc::new(AgentRuntime::new(ai_runtime)), graph);
    (dir, monitor, token, intent_engine, coordination)
}

#[test]
fn a_fresh_task_has_partition_version_zero() {
    let (_dir, monitor, token, intent_engine, coordination) = setup();
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
    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    let market_research = plan
        .nodes
        .iter()
        .find(|n| n.description == "market_research")
        .unwrap()
        .task_id;

    let version = coordination
        .partition_version(&monitor, &token, session, market_research)
        .unwrap();
    assert_eq!(version, 0, "an untouched partition must start at 0");
}

#[test]
fn completing_a_task_really_bumps_its_own_partitions_version() {
    let (_dir, monitor, token, intent_engine, coordination) = setup();
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
    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    let market_research = plan
        .nodes
        .iter()
        .find(|n| n.description == "market_research")
        .unwrap()
        .task_id;
    let legal_formation = plan
        .nodes
        .iter()
        .find(|n| n.description == "legal_formation")
        .unwrap()
        .task_id;

    // Tick 1: market_research (the only ready task) really completes.
    coordination.allocate(&monitor, &token, session).unwrap();

    let after_first_tick = coordination
        .partition_version(&monitor, &token, session, market_research)
        .unwrap();
    assert!(
        after_first_tick >= 1,
        "market_research's own real completion must have bumped its partition, got {after_first_tick}"
    );

    // This workspace's one built-in HTN template is a single connected dependency chain
    // (market_research -> {business_model, branding} -> legal_formation), so every task here
    // shares the same real partition -- a task not yet even dispatched must already show the
    // same non-zero version its sibling's completion produced.
    let legal_version = coordination
        .partition_version(&monitor, &token, session, legal_formation)
        .unwrap();
    assert_eq!(
        legal_version, after_first_tick,
        "tasks in the same real connected dependency chain must share one real partition"
    );
}
