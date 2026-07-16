//! docs/12-multi-agent-coordination.md §8's "Launch my product" worked
//! trace, run against `hyperion-intent`'s own "launch my startup" HTN
//! template (market_research -> {business_model, branding} -> legal_formation)
//! rather than reinventing a ten-Agent fixture — the coordination shape
//! (parallel branches, a dependency chain, a contradiction, a failure that
//! must not corrupt the shared goal) is the same, just fewer branches.

use std::sync::Arc;

use hyperion_agent_runtime::AgentRuntime;
use hyperion_ai_runtime::{
    sign, CancellationToken, InferenceBackend, InferenceRequest, LocalAiRuntime, MockBackend,
    ModelClass, ModelDescriptor, Precision, QuantizedVariant,
};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_coordination::{ConflictResolution, CoordError, CoordinationSession, TaskStatus};
use hyperion_crypto::Keystore;
use hyperion_explainability::ControlState;
use hyperion_intent::{HandleOutcome, IntentEngine, IntentStatus};
use hyperion_knowledge_graph::KnowledgeGraph;

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    IntentEngine,
    CoordinationSession,
) {
    setup_with_backend(Box::new(MockBackend))
}

fn setup_with_backend(
    backend: Box<dyn InferenceBackend>,
) -> (
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
    let ai_runtime = Arc::new(LocalAiRuntime::new(backend, 8_000));

    // A real, signed ModelDescriptor -- needed now that `document.draft`/`web.search` (this
    // trace's own `market_research`/`business_model`/`branding`/`legal_formation` tasks) really
    // call `LocalAiRuntime::infer`, which fails closed with no model registered.
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

/// A real `InferenceBackend` that takes a controlled, real amount of wall-clock time --
/// standing in for a real, slow network round trip to a real cloud model. See
/// `a_ticks_ready_tasks_dispatch_concurrently_not_sequentially` below.
struct SlowBackend {
    delay: std::time::Duration,
}

impl InferenceBackend for SlowBackend {
    fn generate(
        &self,
        _model_id: u64,
        request: &InferenceRequest,
        _cancel: &CancellationToken,
    ) -> String {
        std::thread::sleep(self.delay);
        format!("slow echo: {}", request.prompt)
    }
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

/// `hyperion-intent`'s own named "conflict detection across active graphs" write-back
/// prerequisite: with a real `IntentEngine` wired in via `with_intent_engine`, a real completed
/// dispatch now genuinely transitions the task's own Intent leaf to `IntentStatus::Completed` --
/// not just `TaskStatus::Done` in this crate's own `SharedPlan`.
#[test]
fn a_wired_intent_engine_receives_a_real_completed_status_write_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let intent_engine = Arc::new(IntentEngine::new(graph.clone(), context));
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

    let coordination = CoordinationSession::new(Arc::new(AgentRuntime::new(ai_runtime)), graph)
        .with_intent_engine(intent_engine.clone());

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

    coordination.allocate(&monitor, &token, session).unwrap(); // tick 1: market_research Done

    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    let market_research_id = task_named(&plan, "market_research").task_id;

    let intents = intent_engine.get_graph(&monitor, &token, root).unwrap();
    let market_research_intent = intents.iter().find(|i| i.id == market_research_id).unwrap();
    assert_eq!(
        market_research_intent.status,
        IntentStatus::Completed,
        "a real completed dispatch must write the leaf's own real status back"
    );

    // A leaf that hasn't dispatched yet is genuinely untouched.
    let branding_intent = intents.iter().find(|i| i.predicate == "branding").unwrap();
    assert_eq!(branding_intent.status, IntentStatus::Planned);
}

/// `hyperion-console`'s own real use case: knowing which tasks are *about* to run before the
/// real (potentially slow) dispatch happens, not only after -- so it can announce/spin on them
/// while `allocate` is still blocked on the real work.
#[test]
fn ready_task_descriptions_previews_exactly_what_the_next_allocate_call_will_dispatch() {
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

    let ready = coordination
        .ready_task_descriptions(&monitor, &token, session)
        .unwrap();
    assert_eq!(
        ready,
        vec!["market_research".to_string()],
        "only market_research has no unmet dependency yet"
    );

    coordination.allocate(&monitor, &token, session).unwrap(); // tick 1

    let mut ready = coordination
        .ready_task_descriptions(&monitor, &token, session)
        .unwrap();
    ready.sort();
    assert_eq!(
        ready,
        vec!["branding".to_string(), "business_model".to_string()],
        "both become ready together once market_research is done"
    );

    coordination.allocate(&monitor, &token, session).unwrap(); // tick 2

    let ready = coordination
        .ready_task_descriptions(&monitor, &token, session)
        .unwrap();
    assert_eq!(ready, vec!["legal_formation".to_string()]);

    coordination.allocate(&monitor, &token, session).unwrap(); // tick 3

    let ready = coordination
        .ready_task_descriptions(&monitor, &token, session)
        .unwrap();
    assert!(
        ready.is_empty(),
        "nothing left to dispatch once the whole plan is Done, got: {ready:?}"
    );
}

/// Regression coverage for a real, previously-shipped bottleneck: `allocate` used to dispatch
/// each ready task's real capability one at a time, in a sequential loop -- so a tick with two
/// independent ready tasks (`business_model` and `branding`, this template's own real sibling
/// pair, both depending only on `market_research`) took as long as two real dispatches back to
/// back, however slow each one genuinely was (a real cloud call, in production). Fixed by
/// `allocate`'s own three-phase split (prepare -- under lock, sequential; dispatch -- no lock,
/// concurrent via `std::thread::scope`; apply -- under lock, sequential), which only actually
/// helps because `hyperion_agent_runtime::AgentRuntime::invoke` (and, one layer further down,
/// `hyperion_ai_runtime::LocalAiRuntime::infer`) no longer hold a lock across their own real
/// dispatch either -- see both of those functions' own doc comments.
#[test]
fn a_ticks_ready_tasks_dispatch_concurrently_not_sequentially() {
    let (_dir, monitor, token, intent_engine, coordination) =
        setup_with_backend(Box::new(SlowBackend {
            delay: std::time::Duration::from_millis(200),
        }));
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

    coordination.allocate(&monitor, &token, session).unwrap(); // tick 1: market_research alone

    let start = std::time::Instant::now();
    let records = coordination.allocate(&monitor, &token, session).unwrap(); // tick 2: both ready
    let elapsed = start.elapsed();

    assert_eq!(
        records.len(),
        2,
        "business_model and branding must both become ready in this one tick"
    );
    assert!(
        elapsed < std::time::Duration::from_millis(350),
        "two real 200ms dispatches in one tick took {elapsed:?} -- expected them to genuinely \
         overlap (~200ms total), not run back to back (~400ms total)"
    );
}

/// Regression coverage for a real, previously-shipped bug: `allocate` used to match
/// `InvokeOutcome::Result(_)`, discarding a real capability's own real output the instant it came
/// back — a completed task's own `TaskNode.result` stayed permanently `None`, so nothing
/// downstream (this crate's own callers included) could ever see what a task actually produced,
/// only that it succeeded. `MockBackend` deterministically echoes the whole prompt
/// `dispatch_document_draft`/`dispatch_market_research` build from this session's own `"goal"`
/// (the real utterance) and `"task"` (the predicate) args, so asserting on that echoed text
/// proves genuine, task-specific content survived all the way to `get_plan`, not just that some
/// value is present.
#[test]
fn a_completed_task_carries_its_own_real_capability_result_not_just_a_status() {
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
    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    let market_research = task_named(&plan, "market_research");
    assert_eq!(market_research.status, TaskStatus::Done);

    let result = market_research
        .result
        .as_ref()
        .expect("a Done task must carry the real value its capability dispatch returned");
    let results = result["results"]
        .as_array()
        .expect("web.search's own real result shape");
    let text = results[0].as_str().unwrap();
    assert!(
        text.contains("I need to launch my startup"),
        "expected the real root utterance (this session's own 'goal' arg) to appear in the \
         real, prompt-driven generation, got: {text:?}"
    );
    assert_eq!(
        result["note"], "AI-generated research notes, not a live web search",
        "the honesty caveat must survive all the way to a queryable TaskNode too"
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

/// The real "redo this with more information" verb `hyperion-console`'s own `/redo` meta-command
/// uses. Proves the real round trip end to end: a real, already-`Done` task resets, carries the
/// real extra context into its *next* real dispatch, and comes back `Done` again with that real
/// context genuinely reflected in the regenerated (`MockBackend`-echoed) text.
#[test]
fn amend_task_resets_a_done_task_and_carries_extra_context_into_its_next_real_dispatch() {
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

    coordination.allocate(&monitor, &token, session).unwrap(); // tick 1: market_research Done
    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    assert_eq!(
        task_named(&plan, "market_research").status,
        TaskStatus::Done
    );

    let dependents = coordination
        .amend_task(
            &monitor,
            &token,
            session,
            "market_research",
            "focus on the European market only".to_string(),
        )
        .unwrap();
    assert!(
        dependents.is_empty(),
        "nothing has run yet that depends on market_research, got: {dependents:?}"
    );

    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    let reset = task_named(&plan, "market_research");
    assert_eq!(reset.status, TaskStatus::Unassigned);
    assert!(
        reset.result.is_none(),
        "the now-stale old result must be cleared, not left dangling"
    );

    // Redo it -- market_research has no unmet dependency, so the very next allocate() picks it
    // straight back up.
    let records = coordination.allocate(&monitor, &token, session).unwrap();
    assert_eq!(records.len(), 1);
    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    let redone = task_named(&plan, "market_research");
    assert_eq!(redone.status, TaskStatus::Done);
    let text = redone.result.as_ref().unwrap()["results"][0]
        .as_str()
        .unwrap();
    assert!(
        text.contains("focus on the European market only"),
        "the real extra context must show up in the real, regenerated prompt, got: {text:?}"
    );
}

/// `hyperion-explainability`'s own named "`control.modify` signal plumbing" gap: a real
/// `amend_task` call now genuinely transitions the amended task's most recent real Explanation
/// Record to `ControlState::Modified`, instead of that variant sitting unreachable.
#[test]
fn amend_task_marks_the_tasks_most_recent_explanation_record_as_modified() {
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

    let records = coordination.allocate(&monitor, &token, session).unwrap(); // tick 1: market_research Done
    let plan = coordination.get_plan(&monitor, &token, session).unwrap();
    let market_research_id = task_named(&plan, "market_research").task_id;
    let record = records
        .iter()
        .find(|r| r.task_id == market_research_id)
        .expect("market_research was really dispatched this tick");
    let explanation_id = record.explanation_id;
    assert_eq!(
        coordination
            .explanation(explanation_id)
            .unwrap()
            .control_state,
        ControlState::Completed,
        "before any amendment, the real dispatch's own record is still Completed"
    );

    coordination
        .amend_task(
            &monitor,
            &token,
            session,
            "market_research",
            "focus on the European market only".to_string(),
        )
        .unwrap();

    assert_eq!(
        coordination
            .explanation(explanation_id)
            .unwrap()
            .control_state,
        ControlState::Modified,
        "amending a task must really transition its most recent Explanation Record to Modified"
    );
}

/// Redoing never cascades automatically -- but a caller needs to know which already-`Done` tasks
/// used the now-superseded result, so it can warn (or the user can `/redo` them too).
#[test]
fn amend_task_reports_dependents_that_already_used_the_old_result() {
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

    coordination.allocate(&monitor, &token, session).unwrap(); // tick 1: market_research
    coordination.allocate(&monitor, &token, session).unwrap(); // tick 2: business_model + branding

    let mut dependents = coordination
        .amend_task(&monitor, &token, session, "market_research", String::new())
        .unwrap();
    dependents.sort();
    assert_eq!(
        dependents,
        vec!["branding".to_string(), "business_model".to_string()],
        "both already-Done tasks depend on market_research and must be named"
    );
}

#[test]
fn amend_task_rejects_an_unknown_task_name() {
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

    let result =
        coordination.amend_task(&monitor, &token, session, "not_a_real_task", String::new());
    assert!(
        matches!(result, Err(CoordError::TaskNotFound)),
        "got: {result:?}"
    );
}

/// A task that was never dispatched has no real Explanation Record to transition yet -- amending
/// it must succeed honestly (nothing to mark `Modified`), never error just because there's
/// nothing there.
#[test]
fn amending_a_never_dispatched_task_succeeds_with_no_explanation_to_transition() {
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

    // No allocate() tick has run yet -- business_model has never been dispatched.
    let result = coordination.amend_task(
        &monitor,
        &token,
        session,
        "business_model",
        "steer it".to_string(),
    );
    assert!(result.is_ok(), "got: {result:?}");
}
