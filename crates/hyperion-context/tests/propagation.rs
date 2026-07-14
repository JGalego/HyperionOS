//! docs/07-context-propagation.md: redaction gate, staleness downgrade,
//! signature/replay rejection, and merge conflict surfacing.

use std::collections::HashMap;
use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::{
    merge, Budget, ContextEngine, ContextPropagation, MergeOutcome, PropagationError,
    RedactionAction, RedactionPolicy, Representation, Scope, TrustLevel,
};
use hyperion_crypto::Keystore;
use hyperion_knowledge_graph::KnowledgeGraph;
use serde_json::json;

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    Arc<KnowledgeGraph>,
    Keystore,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, monitor, token, graph, keystore)
}

#[test]
fn category_absent_from_policy_is_redacted_by_default() {
    let (_dir, monitor, token, graph, keystore) = setup();
    let engine = ContextEngine::new(graph.clone());
    let propagation = ContextPropagation::new(graph.clone());

    let node = graph
        .put_node(
            &monitor,
            &token,
            None,
            "financial_detail",
            None,
            json!({"amount": 42}),
        )
        .unwrap();
    let scope = Scope {
        intent_id: "i".into(),
        session_id: "s".into(),
        mentions: Vec::new(),
        anchors: vec![node],
    };
    let bundle = engine
        .assemble(&monitor, &token, &scope, Budget::default())
        .unwrap();

    let policy = RedactionPolicy::new(TrustLevel::SandboxedCapability, HashMap::new());
    let envelope = propagation
        .export(
            &monitor,
            &token,
            &bundle,
            TrustLevel::SandboxedCapability,
            &policy,
            3600,
            &keystore,
        )
        .unwrap();

    assert!(envelope
        .entries
        .iter()
        .all(|e| matches!(e.representation, Representation::RedactedPlaceholder { .. })));
}

#[test]
fn same_boundary_export_uses_by_reference_not_by_value() {
    let (_dir, monitor, token, graph, keystore) = setup();
    let engine = ContextEngine::new(graph.clone());
    let propagation = ContextPropagation::new(graph.clone());

    let node = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"title": "doc"}),
        )
        .unwrap();
    let scope = Scope {
        intent_id: "i".into(),
        session_id: "s".into(),
        mentions: Vec::new(),
        anchors: vec![node],
    };
    let bundle = engine
        .assemble(&monitor, &token, &scope, Budget::default())
        .unwrap();

    let mut rules = HashMap::new();
    rules.insert("document".to_string(), RedactionAction::Pass);
    let policy = RedactionPolicy::new(TrustLevel::SameBoundary, rules);
    let envelope = propagation
        .export(
            &monitor,
            &token,
            &bundle,
            TrustLevel::SameBoundary,
            &policy,
            3600,
            &keystore,
        )
        .unwrap();

    assert!(matches!(
        envelope.entries[0].representation,
        Representation::ByReference { .. }
    ));
}

#[test]
fn cross_boundary_export_materializes_by_value() {
    let (_dir, monitor, token, graph, keystore) = setup();
    let engine = ContextEngine::new(graph.clone());
    let propagation = ContextPropagation::new(graph.clone());

    let node = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"title": "doc"}),
        )
        .unwrap();
    let scope = Scope {
        intent_id: "i".into(),
        session_id: "s".into(),
        mentions: Vec::new(),
        anchors: vec![node],
    };
    let bundle = engine
        .assemble(&monitor, &token, &scope, Budget::default())
        .unwrap();

    let mut rules = HashMap::new();
    rules.insert("document".to_string(), RedactionAction::Pass);
    let policy = RedactionPolicy::new(TrustLevel::RemoteDevice, rules);
    let envelope = propagation
        .export(
            &monitor,
            &token,
            &bundle,
            TrustLevel::RemoteDevice,
            &policy,
            3600,
            &keystore,
        )
        .unwrap();

    assert!(matches!(
        envelope.entries[0].representation,
        Representation::ByValue { .. }
    ));
}

#[test]
fn tampered_envelope_is_rejected_on_import() {
    let (_dir, monitor, token, graph, keystore) = setup();
    let engine = ContextEngine::new(graph.clone());
    let propagation = ContextPropagation::new(graph.clone());

    let node = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"title": "doc"}),
        )
        .unwrap();
    let scope = Scope {
        intent_id: "i".into(),
        session_id: "s".into(),
        mentions: Vec::new(),
        anchors: vec![node],
    };
    let bundle = engine
        .assemble(&monitor, &token, &scope, Budget::default())
        .unwrap();
    let mut rules = HashMap::new();
    rules.insert("document".to_string(), RedactionAction::Pass);
    let policy = RedactionPolicy::new(TrustLevel::RemoteDevice, rules);
    let mut envelope = propagation
        .export(
            &monitor,
            &token,
            &bundle,
            TrustLevel::RemoteDevice,
            &policy,
            3600,
            &keystore,
        )
        .unwrap();

    // Real content tampered after signing, old (now-invalid) signature left in place -- a
    // stronger proof than corrupting the signature bytes themselves: this confirms the real
    // Ed25519 signature actually covers the envelope's content, not just its own presence.
    envelope.bundle_session_id = "forged-session".to_string();
    let result = propagation.import(&monitor, &token, envelope, &keystore.verifying_key());
    assert!(matches!(result, Err(PropagationError::IntegrityFailure)));
}

#[test]
fn an_envelope_signed_by_a_different_keystore_is_rejected() {
    let (_dir, monitor, token, graph, keystore) = setup();
    let engine = ContextEngine::new(graph.clone());
    let propagation = ContextPropagation::new(graph.clone());

    let node = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"title": "doc"}),
        )
        .unwrap();
    let scope = Scope {
        intent_id: "i".into(),
        session_id: "s".into(),
        mentions: Vec::new(),
        anchors: vec![node],
    };
    let bundle = engine
        .assemble(&monitor, &token, &scope, Budget::default())
        .unwrap();
    let mut rules = HashMap::new();
    rules.insert("document".to_string(), RedactionAction::Pass);
    let policy = RedactionPolicy::new(TrustLevel::RemoteDevice, rules);
    let envelope = propagation
        .export(
            &monitor,
            &token,
            &bundle,
            TrustLevel::RemoteDevice,
            &policy,
            3600,
            &keystore,
        )
        .unwrap();

    // A forger with their own real Ed25519 keypair can never produce a signature the real
    // sender's verifying key accepts -- unlike a checksum, which any forger can recompute over
    // altered content without needing any key at all.
    let forger_dir = tempfile::tempdir().unwrap();
    let forger_keystore = Keystore::open_or_create(&forger_dir.path().join("forger.key")).unwrap();
    let result = propagation.import(&monitor, &token, envelope, &forger_keystore.verifying_key());
    assert!(matches!(result, Err(PropagationError::IntegrityFailure)));
}

#[test]
fn replayed_envelope_id_is_rejected_the_second_time() {
    let (_dir, monitor, token, graph, keystore) = setup();
    let engine = ContextEngine::new(graph.clone());
    let propagation = ContextPropagation::new(graph.clone());

    let node = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"title": "doc"}),
        )
        .unwrap();
    let scope = Scope {
        intent_id: "i".into(),
        session_id: "s".into(),
        mentions: Vec::new(),
        anchors: vec![node],
    };
    let bundle = engine
        .assemble(&monitor, &token, &scope, Budget::default())
        .unwrap();
    let mut rules = HashMap::new();
    rules.insert("document".to_string(), RedactionAction::Pass);
    let policy = RedactionPolicy::new(TrustLevel::RemoteDevice, rules);
    let envelope = propagation
        .export(
            &monitor,
            &token,
            &bundle,
            TrustLevel::RemoteDevice,
            &policy,
            3600,
            &keystore,
        )
        .unwrap();

    assert!(propagation
        .import(
            &monitor,
            &token,
            envelope.clone(),
            &keystore.verifying_key()
        )
        .is_ok());
    let result = propagation.import(&monitor, &token, envelope, &keystore.verifying_key());
    assert!(matches!(result, Err(PropagationError::Replayed(_))));
}

#[test]
fn entry_stale_beyond_horizon_is_downgraded_not_trusted() {
    let (_dir, monitor, token, graph, keystore) = setup();
    let engine = ContextEngine::new(graph.clone());
    let propagation = ContextPropagation::new(graph.clone());

    let node = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"title": "v1"}),
        )
        .unwrap();
    let scope = Scope {
        intent_id: "i".into(),
        session_id: "s".into(),
        mentions: Vec::new(),
        anchors: vec![node],
    };
    let bundle = engine
        .assemble(&monitor, &token, &scope, Budget::default())
        .unwrap();
    let mut rules = HashMap::new();
    rules.insert("document".to_string(), RedactionAction::Pass);
    let policy = RedactionPolicy::new(TrustLevel::RemoteDevice, rules);
    // Zero horizon: any staleness detected past the next whole-second
    // boundary counts as beyond horizon.
    let envelope = propagation
        .export(
            &monitor,
            &token,
            &bundle,
            TrustLevel::RemoteDevice,
            &policy,
            0,
            &keystore,
        )
        .unwrap();

    // Simulate a force-push: the underlying object changes after export.
    // Timestamps here are whole-second granularity, so cross a second
    // boundary before checking staleness rather than racing it.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    graph
        .put_node(
            &monitor,
            &token,
            Some(node),
            "document",
            None,
            json!({"title": "v2, force-pushed"}),
        )
        .unwrap();

    let (entries, report) = propagation
        .import(&monitor, &token, envelope, &keystore.verifying_key())
        .unwrap();
    assert!(report
        .per_entry
        .values()
        .any(|s| *s == hyperion_context::FreshnessStatus::StaleBeyondHorizon));
    assert!(entries
        .iter()
        .any(|e| matches!(&e.representation, Representation::RedactedPlaceholder { reason, .. } if reason == "stale")));
}

#[test]
fn merge_picks_up_non_overlapping_edits_and_surfaces_true_conflicts() {
    let (_dir, monitor, token, graph, _keystore) = setup();
    let engine = ContextEngine::new(graph.clone());

    let a = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"title": "a"}),
        )
        .unwrap();
    let b = graph
        .put_node(
            &monitor,
            &token,
            None,
            "document",
            None,
            json!({"title": "b"}),
        )
        .unwrap();

    let scope_a = Scope {
        intent_id: "i".into(),
        session_id: "s".into(),
        mentions: Vec::new(),
        anchors: vec![a],
    };
    let scope_b = Scope {
        intent_id: "i".into(),
        session_id: "s".into(),
        mentions: Vec::new(),
        anchors: vec![b],
    };
    let bundle_a = engine
        .assemble(&monitor, &token, &scope_a, Budget::default())
        .unwrap();
    let bundle_b = engine
        .assemble(&monitor, &token, &scope_b, Budget::default())
        .unwrap();

    // Disjoint node sets: a clean union, no conflicts.
    match merge(&bundle_a.entries, &bundle_b.entries) {
        MergeOutcome::Merged(entries) => {
            let ids: Vec<_> = entries.iter().map(|e| e.node_id).collect();
            assert!(ids.contains(&a));
            assert!(ids.contains(&b));
        }
        MergeOutcome::Conflicts(_) => panic!("disjoint node sets must not conflict"),
    }

    // Same node, but two independently-diverged edits (different
    // generations *and* different content) => a genuine conflict, per
    // docs/07 §Pseudocode `touches_same_field_divergently` — surfaced, not
    // auto-picked. (Equal-generation entries are trusted without a
    // divergence check: this crate's `generation` is tied to the object's
    // actual last-write timestamp, so equal generation already implies
    // equal content — divergence can only arise when generations differ.)
    let mut entry_left = bundle_a.entries[0].clone();
    let mut entry_right = entry_left.clone();
    entry_left.category = "left".to_string();
    entry_right.category = "right".to_string();
    entry_right.generation += 1;
    match merge(&[entry_left], &[entry_right]) {
        MergeOutcome::Conflicts(conflicts) => assert_eq!(conflicts.len(), 1),
        MergeOutcome::Merged(_) => panic!("divergent edits at different generations must conflict"),
    }
}
