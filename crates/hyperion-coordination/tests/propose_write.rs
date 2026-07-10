//! docs/12-multi-agent-coordination.md §5.2's concurrent-write path:
//! optimistic concurrency on a shared fact, and a genuine same-key
//! collision raising a `ConcurrentWrite` conflict rather than a silent
//! last-write-wins.

use std::sync::Arc;

use hyperion_agent_runtime::AgentRuntime;
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_coordination::{ConflictKind, CoordinationSession, WriteOutcome};
use hyperion_intent::HandleOutcome;
use serde_json::json;

fn session_with_root() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    CoordinationSession,
    u64,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(
        hyperion_knowledge_graph::KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap(),
    );
    let context = Arc::new(hyperion_context::ContextEngine::new(graph.clone()));
    let intent_engine = hyperion_intent::IntentEngine::new(graph, context);
    let coordination = CoordinationSession::new(Arc::new(AgentRuntime::new()));

    let root = match intent_engine
        .handle_utterance(&monitor, &token, "I need to launch my startup", "s1")
        .unwrap()
    {
        HandleOutcome::Submitted(id) => id,
        other => panic!("expected Submitted, got {other:?}"),
    };
    let session = coordination
        .create_session(&monitor, &token, &intent_engine, root)
        .unwrap();
    (monitor, token, coordination, session)
}

#[test]
fn first_write_at_version_zero_is_accepted() {
    let (monitor, token, coordination, session) = session_with_root();
    let outcome = coordination
        .propose_write(
            &monitor,
            &token,
            session,
            1,
            "product_name",
            0,
            json!("Nimbus"),
        )
        .unwrap();
    assert!(matches!(outcome, WriteOutcome::Accepted { new_version: 1 }));
}

#[test]
fn write_against_a_stale_base_version_raises_a_concurrent_write_conflict() {
    let (monitor, token, coordination, session) = session_with_root();
    coordination
        .propose_write(
            &monitor,
            &token,
            session,
            1,
            "product_name",
            0,
            json!("Nimbus"),
        )
        .unwrap();

    // Agent 2 read the field before agent 1's write landed (base_version
    // still 0) and now proposes its own value — a genuine collision.
    let outcome = coordination
        .propose_write(
            &monitor,
            &token,
            session,
            2,
            "product_name",
            0,
            json!("Zephyr"),
        )
        .unwrap();
    match outcome {
        WriteOutcome::Conflict(record) => {
            assert_eq!(record.kind, ConflictKind::ConcurrentWrite);
            assert_eq!(record.key, "product_name");
        }
        other => panic!("expected Conflict, got {other:?}"),
    }
}

#[test]
fn writes_to_different_keys_never_conflict() {
    let (monitor, token, coordination, session) = session_with_root();
    let a = coordination
        .propose_write(
            &monitor,
            &token,
            session,
            1,
            "product_name",
            0,
            json!("Nimbus"),
        )
        .unwrap();
    let b = coordination
        .propose_write(
            &monitor,
            &token,
            session,
            2,
            "launch_date",
            0,
            json!("2026-09-01"),
        )
        .unwrap();
    assert!(matches!(a, WriteOutcome::Accepted { .. }));
    assert!(matches!(b, WriteOutcome::Accepted { .. }));
}

#[test]
fn a_correctly_versioned_second_write_after_the_first_is_accepted() {
    let (monitor, token, coordination, session) = session_with_root();
    let WriteOutcome::Accepted { new_version } = coordination
        .propose_write(
            &monitor,
            &token,
            session,
            1,
            "product_name",
            0,
            json!("Nimbus"),
        )
        .unwrap()
    else {
        panic!("expected first write to be accepted");
    };
    let outcome = coordination
        .propose_write(
            &monitor,
            &token,
            session,
            2,
            "product_name",
            new_version,
            json!("Nimbus Pro"),
        )
        .unwrap();
    assert!(
        matches!(outcome, WriteOutcome::Accepted { .. }),
        "a write that read the latest version must succeed"
    );
}
