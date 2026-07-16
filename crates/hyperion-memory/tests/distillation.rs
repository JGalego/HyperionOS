//! docs/08 §5.1's "Working → Episodic distillation via a local model," real for the first time:
//! `MemoryEngine::new_with_ai_runtime` wires a real `LocalAiRuntime` in, and
//! `distill_working_memory` turns a session's real `WorkingMemory` turn buffer into a real,
//! model-generated Episodic summary rather than the previous "caller must summarize it
//! themselves" gap -- with a graceful fallback to a plain verbatim join when real inference
//! can't run.

use std::sync::Arc;

use hyperion_ai_runtime::{
    sign, CancellationToken, InferenceBackend, InferenceRequest, LocalAiRuntime, MockBackend,
    ModelClass, ModelDescriptor, Precision, QuantizedVariant,
};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_memory::{MemoryEngine, MemoryFilter, MemoryTier, WorkingMemory};

/// A real `InferenceBackend` that always answers with a fixed, real, parseable numeric rating --
/// standing in for a real model that actually followed the "respond with only the number"
/// instruction, so `estimate_salience`'s own real parse-and-clamp path is genuinely exercised
/// (`MockBackend`'s own echo never parses as a number).
struct NumericRatingBackend {
    rating: f32,
}

impl InferenceBackend for NumericRatingBackend {
    fn generate(
        &self,
        _model_id: u64,
        _request: &InferenceRequest,
        _cancel: &CancellationToken,
    ) -> String {
        self.rating.to_string()
    }
}

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

fn working_memory_with_turns() -> WorkingMemory {
    let mut wm = WorkingMemory::new("session-1", 10);
    wm.push_turn("user: what's the weather in Lisbon?");
    wm.push_turn("assistant: sunny and 24C");
    wm.push_turn("user: thanks, book me a table for two tonight");
    wm
}

#[test]
fn a_wired_ai_runtime_produces_a_real_model_generated_summary_not_the_verbatim_join() {
    let (_dir, monitor, token, graph) = setup();

    let key_dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&key_dir.path().join("device.key")).unwrap();
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));
    ai_runtime
        .register_model(
            registered_slm_descriptor(&keystore),
            &keystore.verifying_key(),
        )
        .unwrap();

    let engine = MemoryEngine::new_with_ai_runtime(graph.clone(), ai_runtime);
    let working = working_memory_with_turns();

    let id = engine
        .distill_working_memory(&monitor, &token, &working, 0.5, false)
        .unwrap();

    let record = engine
        .query(&monitor, &token, &MemoryFilter::default())
        .unwrap()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap();
    assert_eq!(record.tier, MemoryTier::Episodic);
    let summary = record.content["summary"]
        .as_str()
        .expect("a real ai_runtime-backed summary is a plain string");
    assert!(
        summary.contains("[mock model"),
        "expected MockBackend's own real, deterministic echo response, got: {summary}"
    );
    assert_eq!(record.content["session_id"], "session-1");
}

#[test]
fn no_model_registered_for_the_class_falls_back_to_a_verbatim_join_not_a_failure() {
    let (_dir, monitor, token, graph) = setup();

    // Deliberately no `register_model` call -- `infer()` must return `InfeasibleLocally`, and
    // distillation must fall back rather than failing the whole call.
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));
    let engine = MemoryEngine::new_with_ai_runtime(graph.clone(), ai_runtime);
    let working = working_memory_with_turns();

    let id = engine
        .distill_working_memory(&monitor, &token, &working, 0.5, false)
        .unwrap();

    let record = engine
        .query(&monitor, &token, &MemoryFilter::default())
        .unwrap()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap();
    let summary = record.content["summary"].as_str().unwrap();
    assert_eq!(
        summary,
        "user: what's the weather in Lisbon? | assistant: sunny and 24C | user: thanks, book \
         me a table for two tonight",
        "with no resident model, distillation must fall back to a plain verbatim join, not an \
         error and not a real inference call that couldn't actually run"
    );
}

#[test]
fn no_ai_runtime_wired_keeps_the_verbatim_join_behavior() {
    let (_dir, monitor, token, graph) = setup();
    let engine = MemoryEngine::new(graph.clone());
    let working = working_memory_with_turns();

    let id = engine
        .distill_working_memory(&monitor, &token, &working, 0.5, false)
        .unwrap();

    let record = engine
        .query(&monitor, &token, &MemoryFilter::default())
        .unwrap()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap();
    let summary = record.content["summary"].as_str().unwrap();
    assert!(summary.contains("what's the weather in Lisbon"));
    assert!(summary.contains("book me a table"));
}

#[test]
fn a_distilled_record_really_persists_as_a_real_episodic_knowledge_graph_node() {
    let (_dir, monitor, token, graph) = setup();
    let engine = MemoryEngine::new(graph.clone());
    let working = working_memory_with_turns();

    let id = engine
        .distill_working_memory(&monitor, &token, &working, 0.7, true)
        .unwrap();

    let node = graph.get(&monitor, &token, id).unwrap();
    assert_eq!(node.object_type, "memory_episodic");
    let record = engine
        .query(&monitor, &token, &MemoryFilter::default())
        .unwrap()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap();
    assert_eq!(record.importance, 0.7);
    assert!(record.pinned);
}

/// docs/08 §5.2's own named "model-estimated salience" gap: `I(r) = max(explicit_flag,
/// model_estimated_salience)`, real for the first time. A real model rating higher than the
/// caller's own explicit flag must win.
#[test]
fn a_real_model_estimated_salience_higher_than_the_explicit_flag_wins() {
    let (_dir, monitor, token, graph) = setup();
    let key_dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&key_dir.path().join("device.key")).unwrap();
    let ai_runtime = Arc::new(LocalAiRuntime::new(
        Box::new(NumericRatingBackend { rating: 0.9 }),
        8_000,
    ));
    ai_runtime
        .register_model(
            registered_slm_descriptor(&keystore),
            &keystore.verifying_key(),
        )
        .unwrap();

    let engine = MemoryEngine::new_with_ai_runtime(graph.clone(), ai_runtime);
    let working = working_memory_with_turns();

    let id = engine
        .distill_working_memory(&monitor, &token, &working, 0.2, false)
        .unwrap();

    let record = engine
        .query(&monitor, &token, &MemoryFilter::default())
        .unwrap()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap();
    assert_eq!(
        record.importance, 0.9,
        "a real model-estimated salience (0.9) higher than the explicit flag (0.2) must win, \
         per docs/08 §5.2's own I(r) = max(explicit_flag, model_estimated_salience)"
    );
    assert_eq!(
        record.decay_score, 0.9,
        "the winning importance must really flow into decay_score too"
    );
}

/// The explicit flag must still win when it's the higher of the two -- `max`, never a blind
/// override by whatever the model says.
#[test]
fn the_explicit_flag_wins_when_higher_than_the_real_model_estimate() {
    let (_dir, monitor, token, graph) = setup();
    let key_dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&key_dir.path().join("device.key")).unwrap();
    let ai_runtime = Arc::new(LocalAiRuntime::new(
        Box::new(NumericRatingBackend { rating: 0.1 }),
        8_000,
    ));
    ai_runtime
        .register_model(
            registered_slm_descriptor(&keystore),
            &keystore.verifying_key(),
        )
        .unwrap();

    let engine = MemoryEngine::new_with_ai_runtime(graph.clone(), ai_runtime);
    let working = working_memory_with_turns();

    let id = engine
        .distill_working_memory(&monitor, &token, &working, 0.8, false)
        .unwrap();

    let record = engine
        .query(&monitor, &token, &MemoryFilter::default())
        .unwrap()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap();
    assert_eq!(record.importance, 0.8);
}

/// A real model response that can't be parsed as a number must never fabricate a value --
/// `MockBackend`'s own real echo (never a bare number) is exactly this real, honest case.
#[test]
fn an_unparseable_model_response_never_fabricates_a_salience_value() {
    let (_dir, monitor, token, graph) = setup();
    let key_dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&key_dir.path().join("device.key")).unwrap();
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));
    ai_runtime
        .register_model(
            registered_slm_descriptor(&keystore),
            &keystore.verifying_key(),
        )
        .unwrap();

    let engine = MemoryEngine::new_with_ai_runtime(graph.clone(), ai_runtime);
    let working = working_memory_with_turns();

    let id = engine
        .distill_working_memory(&monitor, &token, &working, 0.4, false)
        .unwrap();

    let record = engine
        .query(&monitor, &token, &MemoryFilter::default())
        .unwrap()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap();
    assert_eq!(
        record.importance, 0.4,
        "an unparseable model response must fall back to the explicit flag alone, never a \
         fabricated number"
    );
}
