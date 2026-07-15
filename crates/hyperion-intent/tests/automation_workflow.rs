//! docs/998-roadmap.md's Resourceful pillar: a plugin-contributed
//! `hyperion_plugin_framework::Contribution::AutomationWorkflow` is a real, live goal template —
//! `IntentEngine::new_with_plugins` really matches it and really decomposes an utterance through
//! it, exactly like a built-in template, and a built-in template still wins when both match.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_crypto::Keystore;
use hyperion_intent::{HandleOutcome, IntentEngine, IntentStatus};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_plugin_framework::{
    sign, AutomationWorkflowContribution, CapabilityGrantRequest, Contribution, Operation,
    PluginManifest, PluginRegistry, QuarantineReason, TrustDepth, WorkflowLeaf,
};

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

fn plan_a_trip_contribution() -> AutomationWorkflowContribution {
    AutomationWorkflowContribution {
        trigger_keywords: vec!["plan a trip".to_string(), "plan my vacation".to_string()],
        root_predicate: "plan_trip".to_string(),
        leaves: vec![
            WorkflowLeaf {
                predicate: "book_flight".to_string(),
                depends_on: vec![],
            },
            WorkflowLeaf {
                predicate: "book_hotel".to_string(),
                depends_on: vec![0],
            },
        ],
    }
}

fn install_workflow(registry: &PluginRegistry, plugin_id: u64) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let (_dir, keystore) = keystore();

    let mut manifest = PluginManifest {
        plugin_id,
        publisher: "acme-workflows".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::AutomationWorkflow(plan_a_trip_contribution())],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::Read,
            scope: "automation-workflow".to_string(),
            justification: "declarative task-graph shape only".to_string(),
        }],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = Some(sign(&manifest, &keystore));

    registry
        .install(
            &mut monitor,
            &root,
            manifest,
            TrustDepth::D0,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();
}

fn setup_with_plugins(
    plugins: Option<Arc<PluginRegistry>>,
) -> (
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
    let engine = IntentEngine::new_with_plugins(graph, context, plugins);
    (dir, monitor, token, engine)
}

#[test]
fn a_plugin_contributed_workflow_really_decomposes_a_matching_utterance() {
    let registry = Arc::new(PluginRegistry::new());
    install_workflow(&registry, 1);
    let (_dir, monitor, token, engine) = setup_with_plugins(Some(registry));

    let root = match engine
        .handle_utterance(
            &monitor,
            &token,
            "help me plan a trip to Japan",
            "session-1",
        )
        .unwrap()
    {
        HandleOutcome::Submitted(id) => id,
        other => panic!("expected Submitted, got {other:?}"),
    };

    let graph = engine.get_graph(&monitor, &token, root).unwrap();
    assert_eq!(graph.len(), 3, "root + 2 leaves");

    let by_predicate = |p: &str| graph.iter().find(|i| i.predicate == p).cloned().unwrap();
    let root_intent = by_predicate("plan_trip");
    assert_eq!(root_intent.status, IntentStatus::Planned);
    assert_eq!(root_intent.confidence, 0.9);

    let book_flight = by_predicate("book_flight");
    assert_eq!(book_flight.status, IntentStatus::Executing, "no dependency");
    let book_hotel = by_predicate("book_hotel");
    assert_eq!(
        book_hotel.status,
        IntentStatus::Planned,
        "waits on book_flight"
    );
}

#[test]
fn without_a_plugin_registry_the_same_utterance_is_ungrounded() {
    let (_dir, monitor, token, engine) = setup_with_plugins(None);

    let root = match engine
        .handle_utterance(
            &monitor,
            &token,
            "help me plan a trip to Japan",
            "session-1",
        )
        .unwrap()
    {
        HandleOutcome::Submitted(id) => id,
        other => panic!("expected Submitted, got {other:?}"),
    };

    let graph = engine.get_graph(&monitor, &token, root).unwrap();
    assert_eq!(
        graph.len(),
        1,
        "no built-in and no plugin template matched, so no decomposition happened"
    );
    assert_eq!(graph[0].predicate, "generic_goal");
}

#[test]
fn a_built_in_template_still_wins_when_both_would_match() {
    let registry = Arc::new(PluginRegistry::new());
    // A plugin contribution that (implausibly) also claims the built-in's own trigger keyword --
    // the built-in must still be the one that matches, per this crate's own "built-ins first"
    // precedent (matching `hyperion-coordination::catalog::best_fit_manifest_with_plugins`).
    let mut monitor = CapabilityMonitor::new();
    let root_token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let (_dir, keystore) = keystore();
    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-workflows".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::AutomationWorkflow(
            AutomationWorkflowContribution {
                trigger_keywords: vec!["launch my".to_string()],
                root_predicate: "impostor_root".to_string(),
                leaves: vec![],
            },
        )],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::Read,
            scope: "automation-workflow".to_string(),
            justification: "declarative task-graph shape only".to_string(),
        }],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = Some(sign(&manifest, &keystore));
    registry
        .install(
            &mut monitor,
            &root_token,
            manifest,
            TrustDepth::D0,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let (_dir2, monitor, token, engine) = setup_with_plugins(Some(registry));
    let root = match engine
        .handle_utterance(&monitor, &token, "I need to launch my startup", "session-1")
        .unwrap()
    {
        HandleOutcome::Submitted(id) => id,
        other => panic!("expected Submitted, got {other:?}"),
    };

    let graph = engine.get_graph(&monitor, &token, root).unwrap();
    assert_eq!(
        graph[0].predicate, "found_company",
        "the built-in template must win over a colliding plugin-contributed one"
    );
}

#[test]
fn quarantining_the_contributing_plugin_stops_it_from_matching() {
    let registry = Arc::new(PluginRegistry::new());
    install_workflow(&registry, 1);
    registry
        .quarantine(1, QuarantineReason::PolicyViolation)
        .unwrap();
    let (_dir, monitor, token, engine) = setup_with_plugins(Some(registry));

    let root = match engine
        .handle_utterance(
            &monitor,
            &token,
            "help me plan a trip to Japan",
            "session-1",
        )
        .unwrap()
    {
        HandleOutcome::Submitted(id) => id,
        other => panic!("expected Submitted, got {other:?}"),
    };

    let graph = engine.get_graph(&monitor, &token, root).unwrap();
    assert_eq!(
        graph.len(),
        1,
        "a quarantined plugin's own workflow must not be matched"
    );
}
