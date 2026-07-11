//! docs/33 §5's `recover_from_crash` — the Phase 8 exit criterion: "a
//! corrupted mid-Agent-execution crash recovers cleanly."

use std::sync::Arc;

use hyperion_agent_runtime::{AgentManifest, AgentRuntime, LifecycleState, TrustTier};
use hyperion_ai_runtime::{LocalAiRuntime, MockBackend};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_recovery::{ActionStatus, RecoveryService, Trigger};

fn setup() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    RecoveryService,
    Arc<KnowledgeGraph>,
    AgentRuntime,
) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let recovery = RecoveryService::new(graph.clone());
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));
    let agent_runtime = AgentRuntime::new(ai_runtime);
    (monitor, root, recovery, graph, agent_runtime)
}

fn manifest() -> AgentManifest {
    AgentManifest {
        specialization: "note-editor".to_string(),
        baseline_capabilities: vec!["document.draft".to_string()],
        requestable_capabilities: Vec::new(),
        trust_tier: TrustTier::System,
    }
}

#[test]
fn an_in_flight_action_at_crash_time_is_rolled_back_and_the_agent_is_respawned() {
    let (monitor, root, recovery, graph, agent_runtime) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "before crash"}),
        )
        .unwrap();

    let instance = agent_runtime
        .spawn(&monitor, &root, manifest(), Some(42))
        .unwrap();

    let rp = recovery
        .recovery_point_create(
            &monitor,
            &root,
            Trigger::PreAgentRun {
                agent_run_id: instance,
            },
            &[node],
            1_000,
        )
        .unwrap();
    let action =
        recovery.record_action_started(rp, vec![node], Some(instance), "agent mid-write", 1_000);

    // The agent partially wrote, then the process "crashed" — the action
    // was never committed or aborted.
    graph
        .put_node(
            &monitor,
            &root,
            Some(node),
            "Note",
            None,
            serde_json::json!({"text": "corrupted mid-write"}),
        )
        .unwrap();

    let recovered = recovery
        .recover_from_crash(&monitor, &root, &agent_runtime, 1_010)
        .unwrap();
    assert_eq!(recovered, vec![action]);

    let restored = graph.get(&monitor, &root, node).unwrap();
    assert_eq!(
        restored.metadata["text"],
        serde_json::json!("before crash"),
        "in-flight writes must be rolled back, never assumed complete"
    );

    let records = recovery.action_records();
    let record = records.iter().find(|r| r.action_id == action).unwrap();
    assert_eq!(record.status, ActionStatus::Aborted);

    // The stale instance is gone; a fresh one exists bound to the same
    // Intent, ready to be re-planned from clean state.
    assert_eq!(
        agent_runtime.state_of(instance),
        Some(LifecycleState::Terminated)
    );
    let fresh_instances: Vec<_> = (instance + 1..instance + 5)
        .filter_map(|id| agent_runtime.describe(id))
        .collect();
    assert_eq!(fresh_instances.len(), 1);
    assert_eq!(fresh_instances[0].bound_intent, Some(42));
    assert_eq!(fresh_instances[0].state, LifecycleState::Bound);
}

#[test]
fn committed_and_aborted_actions_are_left_untouched_by_crash_recovery() {
    let (monitor, root, recovery, graph, agent_runtime) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "v1"}),
        )
        .unwrap();

    let rp = recovery
        .recovery_point_create(&monitor, &root, Trigger::UserRequested, &[node], 1_000)
        .unwrap();
    let committed = recovery.record_action_started(rp, vec![node], None, "finished cleanly", 1_000);
    graph
        .put_node(
            &monitor,
            &root,
            Some(node),
            "Note",
            None,
            serde_json::json!({"text": "v2"}),
        )
        .unwrap();
    recovery.record_action_committed(committed).unwrap();

    let recovered = recovery
        .recover_from_crash(&monitor, &root, &agent_runtime, 1_010)
        .unwrap();
    assert!(recovered.is_empty());

    let current = graph.get(&monitor, &root, node).unwrap();
    assert_eq!(
        current.metadata["text"],
        serde_json::json!("v2"),
        "a cleanly committed action must never be rolled back"
    );
}

#[test]
fn crash_recovery_is_idempotent_across_repeated_calls() {
    let (monitor, root, recovery, graph, agent_runtime) = setup();
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "before"}),
        )
        .unwrap();
    let instance = agent_runtime
        .spawn(&monitor, &root, manifest(), None)
        .unwrap();
    let rp = recovery
        .recovery_point_create(
            &monitor,
            &root,
            Trigger::PreAgentRun {
                agent_run_id: instance,
            },
            &[node],
            1_000,
        )
        .unwrap();
    let action = recovery.record_action_started(rp, vec![node], Some(instance), "mid-write", 1_000);
    graph
        .put_node(
            &monitor,
            &root,
            Some(node),
            "Note",
            None,
            serde_json::json!({"text": "torn write"}),
        )
        .unwrap();

    let first = recovery
        .recover_from_crash(&monitor, &root, &agent_runtime, 1_010)
        .unwrap();
    assert_eq!(first, vec![action]);
    let second = recovery
        .recover_from_crash(&monitor, &root, &agent_runtime, 1_020)
        .unwrap();
    assert!(
        second.is_empty(),
        "an already-aborted action must not be recovered twice"
    );
}
