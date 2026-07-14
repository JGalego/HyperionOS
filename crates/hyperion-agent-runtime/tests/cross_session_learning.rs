//! AUTONOMY_ROADMAP.md's Self-Sustaining pillar's "cross-session learning" slice: a real
//! suspend/auto-resume/backoff-decay history, once wired to a real `hyperion_memory::MemoryEngine`,
//! survives a real process restart -- not just this one running `AgentRuntime`. Proven here by
//! opening the *same real on-disk Knowledge Graph path* twice, with a fresh `MemoryEngine` and a
//! fresh `AgentRuntime` each time and nothing else shared -- the closest a single test process can
//! get to simulating a genuine restart without actually spawning a second real process.

use std::sync::Arc;

use hyperion_agent_runtime::{AgentManifest, AgentRuntime, TrustTier};
use hyperion_ai_runtime::{
    sign, LocalAiRuntime, MockBackend, ModelClass, ModelDescriptor, Precision, QuantizedVariant,
};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_memory::MemoryEngine;
use serde_json::json;

fn manifest() -> AgentManifest {
    AgentManifest {
        specialization: "research".to_string(),
        baseline_capabilities: vec!["web.search".to_string()],
        requestable_capabilities: vec![],
        trust_tier: TrustTier::System,
    }
}

/// Builds one real "session": a real `AgentRuntime` wired to a real `MemoryEngine`, backed by a
/// real Knowledge Graph opened at `graph_path`. Called twice against the *same* `graph_path` to
/// simulate two separate process lifetimes sharing only real, durable, on-disk state.
fn open_session(
    graph_path: &std::path::Path,
) -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    AgentRuntime,
) {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
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

    let graph = Arc::new(KnowledgeGraph::open(graph_path).expect("open the real, shared graph"));
    let memory = Arc::new(MemoryEngine::new(graph));
    let runtime = AgentRuntime::new_with_netstack_and_plugins_and_memory(
        ai_runtime,
        None,
        None,
        Some(memory),
    );

    (monitor, token, runtime)
}

#[test]
fn a_specializations_suspension_history_survives_a_real_restart() {
    let dir = tempfile::tempdir().unwrap();
    let graph_path = dir.path().join("kg.jsonl");

    // "Session" 1: trip the breaker for real, then this whole session (runtime, memory, graph
    // handle) is dropped at the end of this block -- the only thing that survives is the real,
    // on-disk graph file itself.
    {
        let (monitor, token, runtime) = open_session(&graph_path);
        let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();
        assert_eq!(
            runtime.describe(id).unwrap().quota.times_suspended,
            0,
            "a genuinely fresh specialization starts with no remembered history"
        );

        for _ in 0..3 {
            runtime
                .invoke(
                    &monitor,
                    &token,
                    id,
                    "web.search",
                    json!({"force_fail": true}),
                )
                .unwrap();
        }
        assert_eq!(runtime.describe(id).unwrap().quota.times_suspended, 1);
    }

    // "Session" 2: a fresh runtime, fresh memory engine, fresh graph handle -- opened against the
    // exact same real on-disk path. A brand-new instance of the *same* specialization must start
    // already cautious, not from a blank slate.
    {
        let (monitor, token, runtime) = open_session(&graph_path);
        let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();
        assert_eq!(
            runtime.describe(id).unwrap().quota.times_suspended,
            1,
            "a real restart must not erase a specialization's own real, recent suspension history"
        );
    }
}

#[test]
fn a_different_specialization_has_no_remembered_history() {
    let dir = tempfile::tempdir().unwrap();
    let graph_path = dir.path().join("kg.jsonl");

    {
        let (monitor, token, runtime) = open_session(&graph_path);
        let id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();
        for _ in 0..3 {
            runtime
                .invoke(
                    &monitor,
                    &token,
                    id,
                    "web.search",
                    json!({"force_fail": true}),
                )
                .unwrap();
        }
    }

    let (monitor, token, runtime) = open_session(&graph_path);
    let other = AgentManifest {
        specialization: "writer".to_string(),
        baseline_capabilities: vec!["document.draft".to_string()],
        requestable_capabilities: vec![],
        trust_tier: TrustTier::System,
    };
    let id = runtime.spawn(&monitor, &token, other, None).unwrap();
    assert_eq!(
        runtime.describe(id).unwrap().quota.times_suspended,
        0,
        "a specialization's real history must never bleed into an unrelated one"
    );
}
