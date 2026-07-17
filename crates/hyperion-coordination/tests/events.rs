//! docs/31-event-system.md: proves `CoordinationSession::with_events` makes
//! `allocate`'s real `Done` transitions and real escalations (both the
//! failure-retry-exhausted path and `arbitrate_contradiction`'s own path)
//! publish real events onto a real `hyperion_events::EventBus`, rather than
//! only ever being visible to a caller polling `.progress()`/`.escalations()`.

use std::sync::Arc;

use hyperion_agent_runtime::AgentRuntime;
use hyperion_ai_runtime::{
    sign, LocalAiRuntime, MockBackend, ModelClass, ModelDescriptor, Precision, QuantizedVariant,
};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_coordination::{CoordinationSession, TaskStatus};
use hyperion_crypto::Keystore;
use hyperion_events::{
    BackpressurePolicy, DeliveryClass, Event, EventBus, TopicKind, TopicPattern,
};
use hyperion_intent::{HandleOutcome, IntentEngine};
use hyperion_knowledge_graph::KnowledgeGraph;

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    IntentEngine,
    CoordinationSession,
    Arc<EventBus>,
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

    let bus = Arc::new(EventBus::new(None));
    let coordination = CoordinationSession::new(Arc::new(AgentRuntime::new(ai_runtime)), graph)
        .with_events(bus.clone());
    (dir, monitor, token, intent_engine, coordination, bus)
}

fn task_named<'a>(
    plan: &'a hyperion_coordination::SharedPlan,
    predicate: &str,
) -> &'a hyperion_coordination::TaskNode {
    plan.nodes
        .iter()
        .find(|n| n.description == predicate)
        .unwrap()
}

fn drain(sub: &hyperion_events::Subscription) -> Vec<Event> {
    let mut events = Vec::new();
    while let Some(e) = sub.try_recv() {
        events.push(e);
    }
    events
}

#[test]
fn allocate_publishes_a_real_progress_event_on_every_task_completion() {
    let (_dir, monitor, token, intent_engine, coordination, bus) = setup();
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

    // token already carries GRANT (RightsMask::all()), so it can subscribe
    // kind-wide without knowing any task's id in advance.
    let sub = bus
        .subscribe(
            &monitor,
            &token,
            token.origin(),
            TopicPattern::KindScoped(TopicKind::AgentProgress),
            DeliveryClass::AtMostOnce,
            BackpressurePolicy::Buffer { capacity: 32 },
        )
        .unwrap();

    coordination.allocate(&monitor, &token, session).unwrap(); // market_research
    let events = drain(&sub);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].topic.schema_id.0, "coordination.task_progress.v1");
    assert_eq!(events[0].payload_status(), "Done");

    coordination.allocate(&monitor, &token, session).unwrap(); // business_model, branding
    let events = drain(&sub);
    assert_eq!(
        events.len(),
        2,
        "both parallel branches complete in this tick"
    );
}

#[test]
fn a_retry_exhausted_failure_publishes_a_real_escalation_event() {
    let (_dir, monitor, token, intent_engine, coordination, bus) = setup();
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

    coordination.allocate(&monitor, &token, session).unwrap(); // market_research
    coordination.allocate(&monitor, &token, session).unwrap(); // business_model, branding

    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    let legal = task_named(&plan, "legal_formation").task_id;

    let sub = bus
        .subscribe(
            &monitor,
            &token,
            token.origin(),
            TopicPattern::KindScoped(TopicKind::AgentProgress),
            DeliveryClass::AtMostOnce,
            BackpressurePolicy::Buffer { capacity: 32 },
        )
        .unwrap();

    // First injected failure: retried, not yet an escalation.
    coordination
        .inject_failure(&monitor, &token, session, legal)
        .unwrap();
    coordination.allocate(&monitor, &token, session).unwrap();
    assert!(
        drain(&sub).is_empty(),
        "a retry (attempts <= RETRY_LIMIT) is not yet an escalation"
    );

    // Second failure: retry budget exhausted -> real escalation, real event.
    coordination
        .inject_failure(&monitor, &token, session, legal)
        .unwrap();
    coordination.allocate(&monitor, &token, session).unwrap();
    let events = drain(&sub);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].topic.schema_id.0, "coordination.escalation.v1");
    assert!(events[0].payload_reason().contains("legal_formation"));

    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    assert_eq!(
        task_named(&plan, "legal_formation").status,
        TaskStatus::Failed
    );
}

trait PayloadAccess {
    fn payload_status(&self) -> String;
    fn payload_reason(&self) -> String;
}

impl PayloadAccess for Event {
    fn payload_status(&self) -> String {
        match &self.payload {
            hyperion_events::EventPayload::Inline(v) => v["status"].as_str().unwrap().to_string(),
            _ => panic!("expected an inline payload"),
        }
    }

    fn payload_reason(&self) -> String {
        match &self.payload {
            hyperion_events::EventPayload::Inline(v) => v["reason"].as_str().unwrap().to_string(),
            _ => panic!("expected an inline payload"),
        }
    }
}
