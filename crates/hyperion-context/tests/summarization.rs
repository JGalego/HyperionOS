//! docs/06 §2's `summary` inclusion mode, real: `ContextEngine::new_with_ai_runtime` wires a
//! real `LocalAiRuntime` in, and `assemble()`'s own `summary`-mode entries become a real,
//! model-generated summary rather than the previous truncate-to-first-few-fields stand-in --
//! with a graceful fallback to that same stand-in when real inference can't run.

use std::sync::Arc;

use hyperion_ai_runtime::{
    sign, LocalAiRuntime, MockBackend, ModelClass, ModelDescriptor, Precision, QuantizedVariant,
};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::{Budget, ContextEngine, InclusionMode, Scope};
use hyperion_crypto::Keystore;
use hyperion_knowledge_graph::KnowledgeGraph;
use serde_json::json;

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    (dir, monitor, token)
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

/// A large enough `body` that `full_tokens` exceeds `SMALL_ENTRY_TOKENS`, forcing this anchor
/// (whose score is otherwise high enough for `Full`) into `Summary` mode instead -- the same
/// deterministic lever `assemble_never_exceeds_the_token_budget` uses, just for one entry rather
/// than to trigger a budget cutoff.
fn large_metadata() -> serde_json::Value {
    json!({"title": "a document worth summarizing", "body": "x".repeat(2000)})
}

#[test]
fn a_wired_ai_runtime_produces_a_real_model_generated_summary_not_the_truncated_stub() {
    let (dir, monitor, token) = setup();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());

    let key_dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&key_dir.path().join("device.key")).unwrap();
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));
    ai_runtime
        .register_model(
            registered_slm_descriptor(&keystore),
            &keystore.verifying_key(),
        )
        .unwrap();

    let engine = ContextEngine::new_with_ai_runtime(graph.clone(), ai_runtime);

    let anchor = graph
        .put_node(&monitor, &token, None, "document", None, large_metadata())
        .unwrap();
    let scope = Scope {
        intent_id: "intent-1".to_string(),
        session_id: "session-1".to_string(),
        mentions: Vec::new(),
        anchors: vec![anchor],
    };
    let bundle = engine
        .assemble(&monitor, &token, &scope, Budget::default())
        .unwrap();

    let entry = bundle.entries.iter().find(|e| e.node_id == anchor).unwrap();
    assert_eq!(entry.inclusion_mode, InclusionMode::Summary);
    let content = entry
        .content
        .as_str()
        .expect("a real ai_runtime-backed summary is a plain string, not the truncated object");
    assert!(
        content.contains("[mock model"),
        "expected MockBackend's own real, deterministic echo response, got: {content}"
    );
}

#[test]
fn no_model_registered_for_the_class_falls_back_to_the_truncated_stub_not_a_failure() {
    let (dir, monitor, token) = setup();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());

    // Deliberately no `register_model` call -- `infer()` must return `InfeasibleLocally`, and
    // summarization must fall back rather than failing the whole `assemble()`.
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));
    let engine = ContextEngine::new_with_ai_runtime(graph.clone(), ai_runtime);

    let anchor = graph
        .put_node(&monitor, &token, None, "document", None, large_metadata())
        .unwrap();
    let scope = Scope {
        intent_id: "intent-1".to_string(),
        session_id: "session-1".to_string(),
        mentions: Vec::new(),
        anchors: vec![anchor],
    };
    let bundle = engine
        .assemble(&monitor, &token, &scope, Budget::default())
        .unwrap();

    let entry = bundle.entries.iter().find(|e| e.node_id == anchor).unwrap();
    assert_eq!(entry.inclusion_mode, InclusionMode::Summary);
    assert!(
        entry.content.is_object(),
        "with no resident model, summary content must fall back to the truncated object shape, \
         not an error and not a real inference call that couldn't actually run"
    );
}

#[test]
fn no_ai_runtime_wired_keeps_the_original_truncation_behavior() {
    let (dir, monitor, token) = setup();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let engine = ContextEngine::new(graph.clone());

    let anchor = graph
        .put_node(&monitor, &token, None, "document", None, large_metadata())
        .unwrap();
    let scope = Scope {
        intent_id: "intent-1".to_string(),
        session_id: "session-1".to_string(),
        mentions: Vec::new(),
        anchors: vec![anchor],
    };
    let bundle = engine
        .assemble(&monitor, &token, &scope, Budget::default())
        .unwrap();

    let entry = bundle.entries.iter().find(|e| e.node_id == anchor).unwrap();
    assert_eq!(entry.inclusion_mode, InclusionMode::Summary);
    assert_eq!(
        entry.content,
        large_metadata(),
        "with no ai_runtime wired at all, behavior must be identical to before this change \
         (this metadata has only 2 fields, so the truncate-to-first-3-fields stand-in keeps \
         all of it unchanged)"
    );
}
