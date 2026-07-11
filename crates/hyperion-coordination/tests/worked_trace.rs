//! docs/12-multi-agent-coordination.md §8's "Launch my product" worked
//! trace, run against `hyperion-intent`'s own "launch my startup" HTN
//! template (market_research -> {business_model, branding} -> legal_formation)
//! rather than reinventing a ten-Agent fixture — the coordination shape
//! (parallel branches, a dependency chain, a contradiction, a failure that
//! must not corrupt the shared goal) is the same, just fewer branches.

use std::sync::Arc;

use hyperion_agent_runtime::AgentRuntime;
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_coordination::{ConflictResolution, CoordinationSession, TaskStatus};
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
    let intent_engine = IntentEngine::new(graph, context);
    let coordination = CoordinationSession::new(Arc::new(AgentRuntime::new(Arc::new(
        hyperion_ai_runtime::LocalAiRuntime::new(Box::new(hyperion_ai_runtime::MockBackend), 8_000),
    ))));
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
fn create_session_consumes_a_real_execution_ticket_from_submit() {
    let (_dir, monitor, token, intent_engine, coordination) = setup();
    let root = match intent_engine
        .handle_utterance(&monitor, &token, "I need to launch my startup", "s1")
        .unwrap()
    {
        HandleOutcome::Submitted(id) => id,
        other => panic!("expected Submitted, got {other:?}"),
    };

    // The real hand-off docs/05 named as never happening: submit()'s own
    // ExecutionTicket is what create_session now requires, not a bare
    // NodeId a caller could produce without ever calling submit().
    let ticket = intent_engine.submit(&monitor, &token, root).unwrap();
    assert_eq!(ticket.root, root);
    assert_eq!(
        ticket.ready_leaves.len(),
        1,
        "only market_research has no unmet dependency yet"
    );

    let session = coordination
        .create_session(&monitor, &token, &intent_engine, &ticket)
        .unwrap();
    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    assert_eq!(plan.root_intent, ticket.root);
}

#[test]
fn launch_trace_completes_all_tasks_across_ticks_respecting_dependencies() {
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

    // Tick 1: only market_research has no unmet dependency.
    let records = coordination.allocate(&monitor, &token, session).unwrap();
    assert_eq!(records.len(), 1);
    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    assert_eq!(
        task_named(&plan, "market_research").status,
        TaskStatus::Done
    );
    assert_eq!(task_named(&plan, "branding").status, TaskStatus::Unassigned);

    // Tick 2: business_model and branding both become ready.
    let records = coordination.allocate(&monitor, &token, session).unwrap();
    assert_eq!(records.len(), 2);
    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    assert_eq!(task_named(&plan, "business_model").status, TaskStatus::Done);
    assert_eq!(task_named(&plan, "branding").status, TaskStatus::Done);
    assert_eq!(
        task_named(&plan, "legal_formation").status,
        TaskStatus::Unassigned,
        "still waiting on branding's tick to land"
    );

    // Tick 3: legal_formation is now ready.
    let records = coordination.allocate(&monitor, &token, session).unwrap();
    assert_eq!(records.len(), 1);

    assert_eq!(
        coordination.progress(&monitor, &token, session).unwrap(),
        1.0
    );
    assert!(coordination
        .escalations(&monitor, &token, session)
        .unwrap()
        .is_empty());

    // Two specializations (research, writer) cover four tasks — the
    // allocator must reuse instances by load rather than spawning four.
    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    assert_eq!(
        plan.participants.len(),
        2,
        "one research + one writer instance, reused across tasks"
    );
}

#[test]
fn each_allocation_produces_a_real_queryable_explanation_record() {
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

    // Tick 1: market_research succeeds.
    let records = coordination.allocate(&monitor, &token, session).unwrap();
    assert_eq!(records.len(), 1);
    let record = coordination.explanation(records[0].explanation_id).unwrap();
    assert_eq!(
        record.control_state,
        hyperion_explainability::ControlState::Completed
    );
    assert_eq!(record.triggering_intent_id, root.0);
    assert_eq!(record.agent_id, records[0].agent_instance);
    assert!(!record.reasoning_chain.is_empty());

    // Inject a failure on the next ready task and confirm the resulting
    // record reflects the real outcome, not a fixed placeholder.
    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    let business_model = task_named(&plan, "business_model").task_id;
    coordination
        .inject_failure(&monitor, &token, session, business_model)
        .unwrap();
    let records = coordination.allocate(&monitor, &token, session).unwrap();
    let failed_record = records
        .iter()
        .find(|r| r.task_id == business_model)
        .unwrap();
    let record = coordination
        .explanation(failed_record.explanation_id)
        .unwrap();
    assert_eq!(
        record.control_state,
        hyperion_explainability::ControlState::RolledBack,
        "an injected capability failure must roll back its own explanation record"
    );
}

#[test]
fn a_failed_task_is_contained_retried_once_then_escalated_without_stalling_siblings() {
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

    coordination.allocate(&monitor, &token, session).unwrap(); // market_research
    coordination.allocate(&monitor, &token, session).unwrap(); // business_model, branding

    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    let legal = task_named(&plan, "legal_formation").task_id;
    let business_model_status_before = task_named(&plan, "business_model").status;

    // First attempt: injected failure -> retried (requeued), not yet failed.
    coordination
        .inject_failure(&monitor, &token, session, legal)
        .unwrap();
    coordination.allocate(&monitor, &token, session).unwrap();
    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    assert_eq!(
        task_named(&plan, "legal_formation").status,
        TaskStatus::Unassigned,
        "one retry budget remains"
    );
    assert!(coordination
        .escalations(&monitor, &token, session)
        .unwrap()
        .is_empty());

    // Second attempt: fails again -> retry budget exhausted -> escalate.
    coordination
        .inject_failure(&monitor, &token, session, legal)
        .unwrap();
    coordination.allocate(&monitor, &token, session).unwrap();
    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    assert_eq!(
        task_named(&plan, "legal_formation").status,
        TaskStatus::Failed
    );

    let escalations = coordination.escalations(&monitor, &token, session).unwrap();
    assert_eq!(escalations.len(), 1);
    assert!(escalations[0].reason.contains("legal_formation"));

    // Sibling branches untouched by legal_formation's failure — docs/12
    // §8 step 5/§10: "surfacing the root blocker rather than reporting
    // every downstream symptom as an independent failure."
    assert_eq!(
        task_named(&plan, "business_model").status,
        business_model_status_before
    );
    assert_eq!(task_named(&plan, "business_model").status, TaskStatus::Done);
}

#[test]
fn contradictory_subplan_is_arbitrated_by_stated_priority_not_a_coin_flip() {
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
    coordination.allocate(&monitor, &token, session).unwrap();
    coordination.allocate(&monitor, &token, session).unwrap(); // business_model + branding both Done

    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    let branding = task_named(&plan, "branding").task_id;
    let legal = task_named(&plan, "legal_formation").task_id;

    // docs/12 §8 step 4: legal risk outranks branding preference by policy.
    let conflict = coordination
        .arbitrate_contradiction(
            &monitor,
            &token,
            session,
            branding,
            legal,
            &["legal_formation", "branding"],
        )
        .unwrap();
    assert_eq!(conflict.resolution, ConflictResolution::CoordinatorResolved);

    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    assert_eq!(
        task_named(&plan, "branding").status,
        TaskStatus::Unassigned,
        "the lower-priority branch is requeued, not silently overwritten"
    );
    assert!(
        coordination
            .escalations(&monitor, &token, session)
            .unwrap()
            .is_empty(),
        "a resolvable conflict must not also escalate"
    );
}

#[test]
fn unranked_contradiction_escalates_rather_than_guessing() {
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
    coordination.allocate(&monitor, &token, session).unwrap();
    coordination.allocate(&monitor, &token, session).unwrap();

    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    let branding = task_named(&plan, "branding").task_id;
    let business_model = task_named(&plan, "business_model").task_id;

    let conflict = coordination
        .arbitrate_contradiction(
            &monitor,
            &token,
            session,
            branding,
            business_model,
            &["something_else"],
        )
        .unwrap();
    assert_eq!(conflict.resolution, ConflictResolution::Pending);
    assert_eq!(
        coordination
            .escalations(&monitor, &token, session)
            .unwrap()
            .len(),
        1
    );
}
