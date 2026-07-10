//! docs/05-intent-engine.md's worked "launch my startup" example, ambiguity
//! escalation, reconciliation ("cancel that"), and the cycle-rejection
//! Failure Mode.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_intent::{HandleOutcome, IntentEngine, IntentError, IntentStatus};
use hyperion_knowledge_graph::KnowledgeGraph;
use serde_json::json;

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    IntentEngine,
    Arc<KnowledgeGraph>,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let engine = IntentEngine::new(graph.clone(), context);
    (dir, monitor, token, engine, graph)
}

#[test]
fn launch_my_startup_decomposes_into_the_dependency_chain_with_one_ready_leaf() {
    let (_dir, monitor, token, engine, _graph) = setup();
    let root = match engine
        .handle_utterance(&monitor, &token, "I need to launch my startup", "session-1")
        .unwrap()
    {
        HandleOutcome::Submitted(id) => id,
        other => panic!("expected Submitted, got {other:?}"),
    };

    let graph = engine.get_graph(&monitor, &token, root).unwrap();
    assert_eq!(graph.len(), 5, "root + 4 leaves");

    let by_predicate = |p: &str| graph.iter().find(|i| i.predicate == p).cloned().unwrap();
    let root_intent = by_predicate("found_company");
    assert_eq!(root_intent.status, IntentStatus::Planned);

    let market_research = by_predicate("market_research");
    assert_eq!(
        market_research.status,
        IntentStatus::Executing,
        "the only leaf with no dependency must start executing — docs/05's own worked example"
    );

    let legal = by_predicate("legal_formation");
    assert_eq!(
        legal.status,
        IntentStatus::Planned,
        "legal depends on branding and isn't ready yet"
    );

    let ticket = engine.submit(&monitor, &token, root).unwrap();
    assert_eq!(ticket.ready_leaves, vec![market_research.id]);
}

#[test]
fn unrecognized_goal_becomes_a_single_undecomposed_proposed_intent() {
    let (_dir, monitor, token, engine, _graph) = setup();
    let root = match engine
        .handle_utterance(
            &monitor,
            &token,
            "help me pick a birthday gift",
            "session-1",
        )
        .unwrap()
    {
        HandleOutcome::Submitted(id) => id,
        other => panic!("expected Submitted, got {other:?}"),
    };
    let graph = engine.get_graph(&monitor, &token, root).unwrap();
    assert_eq!(
        graph.len(),
        1,
        "no template matched — degrade, never fabricate a plan"
    );
    assert_eq!(graph[0].status, IntentStatus::Proposed);
}

#[test]
fn ambiguous_explicit_mention_escalates_instead_of_guessing() {
    let (_dir, monitor, token, engine, graph) = setup();
    // Neither name is an exact or substring match for the mention below —
    // both land in `hyperion-context`'s word-overlap scoring band, tied at
    // a low, sub-disambiguation-floor confidence, which is exactly the
    // "genuinely ambiguous" case that must escalate rather than guess.
    graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"name": "quarterly marketing budget"}),
        )
        .unwrap();
    graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"name": "marketing budget summary"}),
        )
        .unwrap();

    let outcome = engine
        .handle_utterance(
            &monitor,
            &token,
            "continue working on the marketing budget review",
            "session-1",
        )
        .unwrap();
    match outcome {
        HandleOutcome::NeedsClarification { candidates, .. } => assert!(candidates.len() >= 2),
        HandleOutcome::Submitted(_) => {
            panic!("two equally-plausible documents must escalate, not guess")
        }
    }
}

#[test]
fn unambiguous_explicit_mention_grounds_and_records_provenance() {
    let (_dir, monitor, token, engine, graph) = setup();
    let repo = graph
        .put_node(
            &monitor,
            &token,
            None,
            "repository",
            None,
            json!({"name": "payments-api"}),
        )
        .unwrap();

    let root = match engine
        .handle_utterance(
            &monitor,
            &token,
            "continue working on payments-api",
            "session-1",
        )
        .unwrap()
    {
        HandleOutcome::Submitted(id) => id,
        other => panic!("expected Submitted, got {other:?}"),
    };
    let intent = engine.get_graph(&monitor, &token, root).unwrap().remove(0);
    assert_eq!(intent.grounded_entities, vec![repo]);
    assert!(
        !intent.inferred_fields.is_empty(),
        "a silently-bound mention must be recorded as inferred"
    );
}

#[test]
fn cancel_abandons_the_most_recently_touched_graph_and_its_descendants() {
    let (_dir, monitor, token, engine, _graph) = setup();
    let root = match engine
        .handle_utterance(&monitor, &token, "I need to launch my startup", "session-1")
        .unwrap()
    {
        HandleOutcome::Submitted(id) => id,
        other => panic!("expected Submitted, got {other:?}"),
    };

    engine
        .handle_utterance(&monitor, &token, "actually, cancel that", "session-1")
        .unwrap();

    let graph = engine.get_graph(&monitor, &token, root).unwrap();
    assert!(graph.iter().all(|i| i.status == IntentStatus::Abandoned));
    let root_intent = graph.iter().find(|i| i.id == root).unwrap();
    assert_eq!(
        root_intent.version, 2,
        "cancellation must bump the graph version"
    );
}

#[test]
fn adding_a_dependency_that_would_cycle_is_rejected_before_persisting() {
    let (_dir, monitor, token, engine, _graph) = setup();
    let root = match engine
        .handle_utterance(&monitor, &token, "I need to launch my startup", "session-1")
        .unwrap()
    {
        HandleOutcome::Submitted(id) => id,
        other => panic!("expected Submitted, got {other:?}"),
    };
    let graph = engine.get_graph(&monitor, &token, root).unwrap();
    let market_research = graph
        .iter()
        .find(|i| i.predicate == "market_research")
        .unwrap()
        .id;
    let legal = graph
        .iter()
        .find(|i| i.predicate == "legal_formation")
        .unwrap()
        .id;

    // legal already (transitively) depends on market_research; making
    // market_research depend on legal would close the loop.
    let result = engine.add_dependency(&monitor, &token, market_research, legal);
    assert!(matches!(result, Err(IntentError::CyclicDependency)));

    // A self-dependency must also be rejected outright.
    let result = engine.add_dependency(&monitor, &token, market_research, market_research);
    assert!(matches!(result, Err(IntentError::CyclicDependency)));
}
