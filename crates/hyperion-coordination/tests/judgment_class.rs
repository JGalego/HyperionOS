//! docs/998-roadmap.md's Backlog "Protect the Human" item: a task's `JudgmentClass` is real,
//! assigned from its own predicate at `create_session` time, and `allocate` really records an
//! extra, honest reasoning step for a `TasteOrEmpathy` task -- advisory only, never blocking.

use std::sync::Arc;

use hyperion_agent_runtime::AgentRuntime;
use hyperion_ai_runtime::{
    sign, LocalAiRuntime, MockBackend, ModelClass, ModelDescriptor, Precision, QuantizedVariant,
};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_coordination::{CoordinationSession, JudgmentClass};
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

fn task_named<'a>(
    plan: &'a hyperion_coordination::SharedPlan,
    predicate: &str,
) -> &'a hyperion_coordination::TaskNode {
    plan.nodes
        .iter()
        .find(|n| n.description == predicate)
        .unwrap()
}

#[test]
fn branding_is_classified_taste_or_empathy_everything_else_is_mechanical() {
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

    assert_eq!(
        task_named(&plan, "branding").judgment_class,
        JudgmentClass::TasteOrEmpathy
    );
    for mechanical in ["market_research", "business_model", "legal_formation"] {
        assert_eq!(
            task_named(&plan, mechanical).judgment_class,
            JudgmentClass::Mechanical,
            "'{mechanical}' must default to Mechanical"
        );
    }
}

#[test]
fn a_taste_or_empathy_task_gets_a_real_advisory_reasoning_step_a_mechanical_one_does_not() {
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

    // Tick 1: only market_research (Mechanical) is ready.
    let records = coordination.allocate(&monitor, &token, session).unwrap();
    assert_eq!(records.len(), 1);
    let mechanical_record = coordination
        .explanation(&monitor, &token, records[0].explanation_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        mechanical_record.reasoning_chain.len(),
        1,
        "a Mechanical task must get no extra advisory step, got: {:?}",
        mechanical_record.reasoning_chain
    );

    // Tick 2: business_model (Mechanical) and branding (TasteOrEmpathy) both become ready.
    let records = coordination.allocate(&monitor, &token, session).unwrap();
    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    let branding_task_id = task_named(&plan, "branding").task_id;
    let branding_record = records
        .iter()
        .find(|r| r.task_id == branding_task_id)
        .expect("branding must have been dispatched this tick");
    let explanation = coordination
        .explanation(&monitor, &token, branding_record.explanation_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        explanation.reasoning_chain.len(),
        2,
        "a TasteOrEmpathy task must get a real second, advisory reasoning step, got: {:?}",
        explanation.reasoning_chain
    );
    assert!(
        explanation.reasoning_chain[1]
            .description
            .contains("taste or empathy"),
        "got: {:?}",
        explanation.reasoning_chain[1].description
    );

    let business_model_task_id = task_named(&plan, "business_model").task_id;
    let business_model_record = records
        .iter()
        .find(|r| r.task_id == business_model_task_id)
        .expect("business_model must have been dispatched this tick");
    let explanation = coordination
        .explanation(&monitor, &token, business_model_record.explanation_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        explanation.reasoning_chain.len(),
        1,
        "a Mechanical task dispatched in the same tick must not get the advisory step, got: {:?}",
        explanation.reasoning_chain
    );
}
