//! This crate's own previously-named "agent-runtime/explainability Cargo cycle" gap, closed:
//! `AgentRuntime::with_explainability` wires a real `hyperion_explainability::ExplanationStore`
//! in, and `AgentRuntime::invoke` opens/closes a real Explanation Record around its own dispatch.

use std::sync::Arc;

use hyperion_agent_runtime::{AgentManifest, AgentRuntime, InvokeOutcome, TrustTier};
use hyperion_ai_runtime::{
    sign, LocalAiRuntime, MockBackend, ModelClass, ModelDescriptor, Precision, QuantizedVariant,
};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_explainability::{ControlState, ExplanationStore};

fn manifest() -> AgentManifest {
    AgentManifest {
        specialization: "research".to_string(),
        baseline_capabilities: vec!["web.search".to_string()],
        requestable_capabilities: Vec::new(),
        trust_tier: TrustTier::System,
    }
}

fn setup() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    AgentRuntime,
    Arc<ExplanationStore>,
) {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));

    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
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

    let explainability = Arc::new(ExplanationStore::new());
    let runtime = AgentRuntime::new(ai_runtime).with_explainability(explainability.clone());
    (monitor, token, runtime, explainability)
}

#[test]
fn a_successful_invoke_produces_a_real_completed_explanation_record() {
    let (monitor, token, runtime, explainability) = setup();
    let instance_id = runtime
        .spawn(&monitor, &token, manifest(), Some(99))
        .unwrap();

    let outcome = runtime
        .invoke(
            &monitor,
            &token,
            instance_id,
            "web.search",
            serde_json::json!({"query": "hyperion os"}),
        )
        .unwrap();
    assert!(matches!(outcome, InvokeOutcome::Result(_)));

    let records = explainability.trace_intent(&monitor, &token, 99).unwrap();
    assert_eq!(
        records.len(),
        1,
        "the real bound_intent (99) must be a genuine correlation, not lost"
    );
    assert_eq!(records[0].control_state, ControlState::Completed);
    assert_eq!(records[0].agent_id, instance_id);
    assert_eq!(records[0].capability_ref, "web.search");
    assert!(!records[0].reasoning_chain.is_empty());
}

#[test]
fn a_failed_invoke_produces_a_real_rolled_back_explanation_record() {
    let (monitor, token, runtime, explainability) = setup();
    let instance_id = runtime
        .spawn(&monitor, &token, manifest(), Some(1))
        .unwrap();

    let outcome = runtime
        .invoke(
            &monitor,
            &token,
            instance_id,
            "web.search",
            serde_json::json!({"force_fail": true}),
        )
        .unwrap();
    assert!(matches!(outcome, InvokeOutcome::Failed(_)));

    let records = explainability.trace_intent(&monitor, &token, 1).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].control_state, ControlState::RolledBack);
}

#[test]
fn with_no_bound_intent_the_record_still_opens_under_a_real_sentinel_zero() {
    let (monitor, token, runtime, explainability) = setup();
    let instance_id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();

    runtime
        .invoke(
            &monitor,
            &token,
            instance_id,
            "web.search",
            serde_json::json!({"query": "x"}),
        )
        .unwrap();

    let records = explainability.trace_intent(&monitor, &token, 0).unwrap();
    assert_eq!(records.len(), 1);
}

#[test]
fn with_no_explainability_wired_invoke_behaves_exactly_as_before() {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));

    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
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

    // Deliberately not calling `.with_explainability(...)` -- this is the pre-existing default.
    let runtime = AgentRuntime::new(ai_runtime);
    let instance_id = runtime.spawn(&monitor, &token, manifest(), None).unwrap();

    let outcome = runtime
        .invoke(
            &monitor,
            &token,
            instance_id,
            "web.search",
            serde_json::json!({"query": "x"}),
        )
        .unwrap();
    assert!(matches!(outcome, InvokeOutcome::Result(_)));
}
