//! docs/05 §2's "generative decomposition" fallback, made real: an utterance matching no curated
//! or plugin-contributed HTN template now gets one real, model-generated ordered step list when
//! a real `LocalAiRuntime` is wired, instead of always degrading to a single undecomposed root
//! Intent.

use std::sync::Arc;

use hyperion_ai_runtime::{
    sign, LocalAiRuntime, MockBackend, ModelClass, ModelDescriptor, Precision, QuantizedVariant,
};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_crypto::Keystore;
use hyperion_intent::{HandleOutcome, IntentEngine, IntentStatus};
use hyperion_knowledge_graph::KnowledgeGraph;

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    Arc<KnowledgeGraph>,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    (dir, monitor, token, graph)
}

fn registered_slm_descriptor(keystore: &Keystore) -> ModelDescriptor {
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
    descriptor.signature = Some(sign(&descriptor, keystore));
    descriptor
}

#[test]
fn a_wired_ai_runtime_produces_a_real_generated_plan_for_an_unmatched_utterance() {
    let (_dir, monitor, token, graph) = setup();
    let context = Arc::new(ContextEngine::new(graph.clone()));

    let key_dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&key_dir.path().join("device.key")).unwrap();
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));
    ai_runtime
        .register_model(
            registered_slm_descriptor(&keystore),
            &keystore.verifying_key(),
        )
        .unwrap();

    let engine =
        IntentEngine::new_with_plugins_and_ai_runtime(graph, context, None, Some(ai_runtime));

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

    let subtree = engine.get_graph(&monitor, &token, root).unwrap();
    let root_intent = subtree.iter().find(|i| i.id == root).unwrap();
    assert_eq!(
        root_intent.status,
        IntentStatus::Planned,
        "a real generated plan is no longer just Proposed"
    );
    assert!(
        (0.6 - root_intent.confidence).abs() < f32::EPSILON,
        "a model-generated plan is real but lower-confidence than a curated template match, got \
         confidence {}",
        root_intent.confidence
    );
    assert!(
        subtree.len() > 1,
        "expected real generated leaves, not a single undecomposed root, got: {subtree:?}"
    );
    assert!(
        subtree
            .iter()
            .any(|i| i.parent == Some(root) && i.predicate.contains("mock_model")),
        "expected at least one leaf predicate derived from the real (mocked) model response, \
         got: {subtree:?}"
    );
}

#[test]
fn with_no_model_registered_generation_falls_back_to_the_single_undecomposed_root() {
    let (_dir, monitor, token, graph) = setup();
    let context = Arc::new(ContextEngine::new(graph.clone()));
    // A real, wired LocalAiRuntime with no model ever registered for ModelClass::Slm --
    // `infer` fails with `InfeasibleLocally`, so generation must degrade honestly.
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));

    let engine =
        IntentEngine::new_with_plugins_and_ai_runtime(graph, context, None, Some(ai_runtime));

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

    let subtree = engine.get_graph(&monitor, &token, root).unwrap();
    assert_eq!(
        subtree.len(),
        1,
        "no model resident to generate from -- must degrade to the pre-existing fallback, not \
         fabricate a plan"
    );
    assert_eq!(subtree[0].status, IntentStatus::Proposed);
}
