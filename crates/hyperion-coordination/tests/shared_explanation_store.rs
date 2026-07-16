//! docs/998-roadmap.md's own named "workspace-wide, shared Explanation Record store" gap, closed
//! for a caller that wants it: `CoordinationSession::new_with_shared_explanations` and
//! `FederationHub::new_with_shared_explanations` can share one real `hyperion_explainability::
//! ExplanationStore` -- proven here with two genuinely independent, real owners writing into the
//! very same store, with no `action_id` collision, and both real records findable through the
//! same store.

use std::sync::Arc;

use hyperion_agent_runtime::{AgentManifest, TrustTier};
use hyperion_ai_runtime::{
    sign, LocalAiRuntime, MockBackend, ModelClass, ModelDescriptor, Precision, QuantizedVariant,
};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_coordination::CoordinationSession;
use hyperion_crypto::Keystore;
use hyperion_explainability::ExplanationStore;
use hyperion_federation::{FederationHub, FederationTrustTier};
use hyperion_intent::{HandleOutcome, IntentEngine};
use hyperion_knowledge_graph::KnowledgeGraph;

#[test]
fn two_genuinely_independent_owners_share_one_store_with_no_action_id_collision() {
    let shared_store = Arc::new(ExplanationStore::new());

    // Owner 1: a real hyperion-coordination session.
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
        .unwrap();

    let coordination = CoordinationSession::new_with_shared_explanations(
        Arc::new(hyperion_agent_runtime::AgentRuntime::new(ai_runtime)),
        graph,
        Arc::clone(&shared_store),
    );

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
    // Tick 1: market_research (the only ready task) dispatches for real, opening a real
    // Explanation Record in the *shared* store under this real Intent id.
    let coordination_records = coordination.allocate(&monitor, &token, session).unwrap();
    assert_eq!(coordination_records.len(), 1);
    let coordination_action_id = shared_store
        .get(coordination_records[0].explanation_id)
        .unwrap()
        .action_id;

    // Owner 2: a real, genuinely independent hyperion-federation hub, sharing the exact same
    // store -- not a second, disconnected one.
    let federation_hub = FederationHub::new_with_shared_explanations(
        Keystore::ephemeral(),
        Arc::clone(&shared_store),
    );
    federation_hub
        .join_device(&monitor, &token, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    let agent = federation_hub
        .spawn_agent(
            &monitor,
            &token,
            1,
            AgentManifest {
                specialization: "navigation".to_string(),
                baseline_capabilities: vec!["web.search".to_string()],
                requestable_capabilities: vec![],
                trust_tier: TrustTier::System,
            },
            None,
            1_000,
            60,
        )
        .unwrap();
    // Uses the exact same real Intent id coordination's own dispatch used above -- a genuine
    // cross-owner correlation under one shared triggering_intent_id, not a coincidence.
    federation_hub
        .invoke_agent(
            &monitor,
            &token,
            agent,
            "web.search",
            serde_json::json!({"query": "hyperion os"}),
            root.0,
            1_010,
        )
        .unwrap();

    // Both owners' real records are findable through the one shared store, correlated by the
    // same real Intent id.
    let all_records_for_intent = shared_store.trace_intent(root.0);
    assert_eq!(
        all_records_for_intent.len(),
        2,
        "the shared store must hold one real record from each genuinely independent owner, got: \
         {all_records_for_intent:?}"
    );

    // The real property `ExplanationStore::next_action_id` exists to guarantee: two different
    // owners' action_ids, minted from the one shared counter, never collide.
    let federation_action_id = all_records_for_intent
        .iter()
        .find(|r| r.action_id != coordination_action_id)
        .expect("federation's own record must have a distinct action_id")
        .action_id;
    assert_ne!(
        coordination_action_id, federation_action_id,
        "two genuinely independent owners sharing one store must never mint colliding action_ids"
    );
}
