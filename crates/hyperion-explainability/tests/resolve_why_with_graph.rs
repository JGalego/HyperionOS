//! `resolve_why_with_graph` -- this crate's own previously-unnamed gap: `ReasoningStep.inputs_ref`/
//! `output_ref` and `EvidenceRef.object_id` are real `hyperion_knowledge_graph::NodeId`s, but
//! nothing here ever turned one into something a person could read. These tests prove a real
//! Knowledge Graph node's own `display_label` shows up in the resolved view, that an unresolvable
//! reference degrades honestly instead of erroring, and that resolution reaches every depth of a
//! multi-agent parent chain, not only the root.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_explainability::{
    resolve_why_with_graph, ConfidenceMethod, ConfidenceScore, Depth, EvidenceRef,
    ExplanationStore, ReasoningStep,
};
use hyperion_knowledge_graph::KnowledgeGraph;
use serde_json::json;

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    ExplanationStore,
    KnowledgeGraph,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    (dir, monitor, root, ExplanationStore::new(), graph)
}

#[test]
fn a_real_input_reference_resolves_to_its_real_display_label() {
    let (_dir, monitor, root, store, graph) = setup();

    let subject = graph
        .put_node(
            &monitor,
            &root,
            None,
            "intent",
            None,
            json!({"predicate": "market_research"}),
        )
        .unwrap();

    let id = store
        .begin(&monitor, &root, 42, 7, 1, "web.research", vec![], 1_000)
        .unwrap();
    store
        .append_step(
            &monitor,
            &root,
            id,
            ReasoningStep {
                step_index: 0,
                description: "chose web.research for the intent".to_string(),
                capability_ref: Some("web.research".to_string()),
                inputs_ref: vec![subject],
                output_ref: None,
            },
            vec![],
        )
        .unwrap();

    let view = resolve_why_with_graph(&store, &graph, &monitor, &root, 42, Depth::Full)
        .unwrap()
        .unwrap();

    assert_eq!(view.resolved_reasoning_chain.len(), 1);
    assert_eq!(
        view.resolved_reasoning_chain[0].inputs,
        vec!["a planned task: market_research".to_string()]
    );
}

#[test]
fn a_dangling_reference_resolves_honestly_instead_of_erroring() {
    let (_dir, monitor, root, store, graph) = setup();

    let id = store
        .begin(&monitor, &root, 42, 7, 1, "web.research", vec![], 1_000)
        .unwrap();
    store
        .append_step(
            &monitor,
            &root,
            id,
            ReasoningStep {
                step_index: 0,
                description: "referenced something that no longer exists".to_string(),
                capability_ref: None,
                inputs_ref: vec![],
                output_ref: Some(hyperion_storage::ObjectId(9_999)),
            },
            vec![],
        )
        .unwrap();

    let view = resolve_why_with_graph(&store, &graph, &monitor, &root, 42, Depth::Full)
        .unwrap()
        .unwrap();

    assert_eq!(
        view.resolved_reasoning_chain[0].output,
        Some("a reference that's no longer available".to_string())
    );
}

#[test]
fn evidence_resolves_alongside_its_real_excerpt_and_weight() {
    let (_dir, monitor, root, store, graph) = setup();

    let evidence_node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "task_result",
            None,
            json!({"draft": "a quarterly plan"}),
        )
        .unwrap();

    let id = store
        .begin(&monitor, &root, 42, 7, 1, "document.draft", vec![], 1_000)
        .unwrap();
    store
        .append_step(
            &monitor,
            &root,
            id,
            ReasoningStep {
                step_index: 0,
                description: "drafted from evidence".to_string(),
                capability_ref: None,
                inputs_ref: vec![],
                output_ref: None,
            },
            vec![EvidenceRef {
                object_id: evidence_node,
                excerpt_or_summary: "early draft".to_string(),
                weight: 0.9,
            }],
        )
        .unwrap();

    let view = resolve_why_with_graph(&store, &graph, &monitor, &root, 42, Depth::Full)
        .unwrap()
        .unwrap();

    assert_eq!(view.resolved_evidence.len(), 1);
    assert_eq!(
        view.resolved_evidence[0].label,
        "a result: a quarterly plan"
    );
    assert_eq!(view.resolved_evidence[0].excerpt_or_summary, "early draft");
    assert_eq!(view.resolved_evidence[0].weight, 0.9);
}

#[test]
fn resolution_reaches_every_depth_of_a_multi_agent_parent_chain() {
    let (_dir, monitor, root, store, graph) = setup();

    let coordinator_subject = graph
        .put_node(
            &monitor,
            &root,
            None,
            "intent",
            None,
            json!({"predicate": "coordination.plan"}),
        )
        .unwrap();
    let worker_subject = graph
        .put_node(
            &monitor,
            &root,
            None,
            "intent",
            None,
            json!({"predicate": "document.draft"}),
        )
        .unwrap();

    let coordinator = store
        .begin(&monitor, &root, 1, 7, 1, "coordination.plan", vec![], 1_000)
        .unwrap();
    store
        .append_step(
            &monitor,
            &root,
            coordinator,
            ReasoningStep {
                step_index: 0,
                description: "planned".to_string(),
                capability_ref: None,
                inputs_ref: vec![coordinator_subject],
                output_ref: None,
            },
            vec![],
        )
        .unwrap();

    let worker = store
        .begin(&monitor, &root, 2, 7, 2, "document.draft", vec![], 1_005)
        .unwrap();
    store
        .append_step(
            &monitor,
            &root,
            worker,
            ReasoningStep {
                step_index: 0,
                description: "drafted".to_string(),
                capability_ref: None,
                inputs_ref: vec![worker_subject],
                output_ref: None,
            },
            vec![],
        )
        .unwrap();
    store
        .link_parent(&monitor, &root, coordinator, worker)
        .unwrap();

    let root_view = resolve_why_with_graph(&store, &graph, &monitor, &root, 1, Depth::Full)
        .unwrap()
        .unwrap();

    assert_eq!(
        root_view.resolved_reasoning_chain[0].inputs,
        vec!["a planned task: coordination.plan".to_string()]
    );
    assert_eq!(root_view.parents.len(), 1);
    assert_eq!(
        root_view.parents[0].resolved_reasoning_chain[0].inputs,
        vec!["a planned task: document.draft".to_string()]
    );
}

#[test]
fn headline_depth_never_populates_resolved_fields_since_there_is_no_full_record_to_resolve() {
    let (_dir, monitor, root, store, graph) = setup();

    let id = store
        .begin(&monitor, &root, 42, 7, 1, "web.research", vec![], 1_000)
        .unwrap();
    store
        .set_confidence(
            &monitor,
            &root,
            id,
            ConfidenceScore {
                value: 0.6,
                method: ConfidenceMethod::Heuristic,
            },
            vec![],
        )
        .unwrap();

    let view = resolve_why_with_graph(&store, &graph, &monitor, &root, 42, Depth::Headline)
        .unwrap()
        .unwrap();

    assert!(view.resolved_reasoning_chain.is_empty());
    assert!(view.resolved_evidence.is_empty());
}
