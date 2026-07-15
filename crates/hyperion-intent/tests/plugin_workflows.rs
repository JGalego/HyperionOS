//! docs/998-roadmap.md's Resourceful pillar: a plugin-contributed `Contribution::AutomationWorkflow`
//! is a real, live goal-template registry — a real utterance really matches it, decomposes into
//! its real leaves with real dependency edges, exactly like the built-in `TEMPLATES` roster
//! already does.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_crypto::Keystore;
use hyperion_intent::{HandleOutcome, IntentEngine};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_plugin_framework::{
    sign, AutomationWorkflowContribution, CapabilityGrantRequest, Contribution, Operation,
    PluginManifest, PluginRegistry, TrustDepth, WorkflowLeaf,
};

fn install_recipe_workflow(registry: &PluginRegistry) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();

    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-workflows".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::AutomationWorkflow(
            AutomationWorkflowContribution {
                trigger_keywords: vec!["plan a dinner party".to_string()],
                root_predicate: "plan_dinner_party".to_string(),
                leaves: vec![
                    WorkflowLeaf {
                        predicate: "pick_recipes".to_string(),
                        depends_on: vec![],
                    },
                    WorkflowLeaf {
                        predicate: "buy_ingredients".to_string(),
                        depends_on: vec![0],
                    },
                ],
            },
        )],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::Read,
            scope: "automation-workflow".to_string(),
            justification: "descriptive workflow shape only".to_string(),
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

fn engine_with(
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
fn without_a_registry_an_unrecognized_utterance_falls_back_to_generic_goal() {
    let (_dir, monitor, token, engine) = engine_with(None);
    let outcome = engine
        .handle_utterance(&monitor, &token, "plan a dinner party for six", "s1")
        .unwrap();
    let HandleOutcome::Submitted(root) = outcome else {
        panic!("expected Submitted");
    };
    let graph_nodes = engine.trace_intent(root.0);
    // No explanation record means it never decomposed -- the generic_goal fallback path.
    assert!(graph_nodes.is_empty());
}

#[test]
fn a_plugin_contributed_workflow_really_decomposes_a_matching_utterance() {
    let registry = Arc::new(PluginRegistry::new());
    install_recipe_workflow(&registry);
    let (_dir, monitor, token, engine) = engine_with(Some(registry));

    let outcome = engine
        .handle_utterance(&monitor, &token, "please plan a dinner party for six", "s1")
        .unwrap();
    let HandleOutcome::Submitted(root) = outcome else {
        panic!("expected Submitted");
    };

    let records = engine.trace_intent(root.0);
    assert_eq!(
        records.len(),
        1,
        "a real decomposition must open exactly one explanation record"
    );
    let steps = &records[0].reasoning_chain;
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].capability_ref.as_deref(), Some("pick_recipes"));
    assert_eq!(steps[1].capability_ref.as_deref(), Some("buy_ingredients"));
}
