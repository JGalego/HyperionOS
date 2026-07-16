//! docs/998-roadmap.md's Backlog "Protect the Human" item: an opt-in, per-session pause before
//! `IntentEngine::handle_utterance` decomposes a matched goal, real end to end via
//! `IntentEngine::set_think_mode`/`proceed_with_decomposition`.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_intent::{HandleOutcome, IntentEngine, IntentError};
use hyperion_knowledge_graph::KnowledgeGraph;

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    IntentEngine,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let engine = IntentEngine::new(graph, context);
    (dir, monitor, token, engine)
}

#[test]
fn think_mode_is_off_by_default_and_decomposes_immediately() {
    let (_dir, monitor, token, engine) = setup();
    assert!(!engine.is_think_mode("session-1"));

    let root = match engine
        .handle_utterance(&monitor, &token, "I need to launch my startup", "session-1")
        .unwrap()
    {
        HandleOutcome::Submitted(id) => id,
        other => panic!("expected Submitted with think mode off, got {other:?}"),
    };
    let graph = engine.get_graph(&monitor, &token, root).unwrap();
    assert_eq!(graph.len(), 5, "root + 4 leaves, decomposed immediately");
}

#[test]
fn think_mode_on_pauses_decomposition_until_proceed_is_called() {
    let (_dir, monitor, token, engine) = setup();
    engine.set_think_mode("session-1", true);
    assert!(engine.is_think_mode("session-1"));

    let root = match engine
        .handle_utterance(&monitor, &token, "I need to launch my startup", "session-1")
        .unwrap()
    {
        HandleOutcome::PendingThink(id) => id,
        other => panic!("expected PendingThink with think mode on, got {other:?}"),
    };

    // Not decomposed yet -- just the bare root.
    let graph = engine.get_graph(&monitor, &token, root).unwrap();
    assert_eq!(
        graph.len(),
        1,
        "a paused decomposition must not have created any leaves yet"
    );

    let outcome = engine
        .proceed_with_decomposition(&monitor, &token, root)
        .unwrap();
    assert!(matches!(outcome, HandleOutcome::Submitted(id) if id == root));

    let graph = engine.get_graph(&monitor, &token, root).unwrap();
    assert_eq!(
        graph.len(),
        5,
        "root + 4 leaves, decomposed once explicitly told to proceed"
    );
}

#[test]
fn proceed_with_decomposition_on_an_unknown_root_is_not_found() {
    let (_dir, monitor, token, engine) = setup();
    let root = match engine
        .handle_utterance(&monitor, &token, "I need to launch my startup", "session-1")
        .unwrap()
    {
        HandleOutcome::Submitted(id) => id,
        other => panic!("expected Submitted, got {other:?}"),
    };

    // Already decomposed immediately (think mode was never on) -- nothing pending on it.
    let result = engine.proceed_with_decomposition(&monitor, &token, root);
    assert!(matches!(result, Err(IntentError::NotFound)));
}

#[test]
fn proceed_with_decomposition_consumes_the_pending_entry_exactly_once() {
    let (_dir, monitor, token, engine) = setup();
    engine.set_think_mode("session-1", true);
    let root = match engine
        .handle_utterance(&monitor, &token, "I need to launch my startup", "session-1")
        .unwrap()
    {
        HandleOutcome::PendingThink(id) => id,
        other => panic!("expected PendingThink, got {other:?}"),
    };

    engine
        .proceed_with_decomposition(&monitor, &token, root)
        .unwrap();
    let second_call = engine.proceed_with_decomposition(&monitor, &token, root);
    assert!(
        matches!(second_call, Err(IntentError::NotFound)),
        "a pending decomposition must only ever be resolved once"
    );
}

#[test]
fn think_mode_is_scoped_per_session() {
    let (_dir, monitor, token, engine) = setup();
    engine.set_think_mode("session-thinking", true);

    let paused = engine
        .handle_utterance(
            &monitor,
            &token,
            "I need to launch my startup",
            "session-thinking",
        )
        .unwrap();
    assert!(matches!(paused, HandleOutcome::PendingThink(_)));

    let immediate = engine
        .handle_utterance(
            &monitor,
            &token,
            "I need to launch my startup",
            "session-not-thinking",
        )
        .unwrap();
    assert!(matches!(immediate, HandleOutcome::Submitted(_)));
}

#[test]
fn set_think_mode_off_stops_pausing_future_utterances() {
    let (_dir, monitor, token, engine) = setup();
    engine.set_think_mode("session-1", true);
    engine.set_think_mode("session-1", false);
    assert!(!engine.is_think_mode("session-1"));

    let outcome = engine
        .handle_utterance(&monitor, &token, "I need to launch my startup", "session-1")
        .unwrap();
    assert!(matches!(outcome, HandleOutcome::Submitted(_)));
}
