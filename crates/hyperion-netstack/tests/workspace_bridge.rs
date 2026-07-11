//! docs/19 §10's human-in-the-loop disambiguation: a `needs_review`
//! `SemanticObjectRef` compiles into a real `hyperion-workspace` Workspace,
//! not just a flag sitting on the node's metadata.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_netstack::{
    present_disambiguation_as_workspace, DomainEgressGrant, EntityType, FetchedPage,
    FreshnessPolicy, MockExtractionBackend, MockFetchBackend, NetstackHub, StructuredSignal,
    WebResolutionRequest,
};
use hyperion_workspace::WorkspaceCompiler;

fn setup() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    NetstackHub,
    Arc<MockFetchBackend>,
) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let fetch = Arc::new(MockFetchBackend::new());
    let hub = NetstackHub::new(
        graph,
        Box::new(fetch.clone()),
        Box::new(MockExtractionBackend),
    );
    (monitor, root, hub, fetch)
}

fn ample_grant() -> DomainEgressGrant {
    DomainEgressGrant {
        domain_patterns: vec!["example.com".to_string()],
        rate_limit_per_window: 100,
        window_secs: 60,
        max_depth: 5,
        expiry: None,
    }
}

fn request(origin: &str) -> WebResolutionRequest {
    WebResolutionRequest {
        origin: origin.to_string(),
        agent_id: 7,
        purpose: "research".to_string(),
        freshness: FreshnessPolicy::UseCache,
        depth: 0,
    }
}

#[test]
fn a_needs_review_result_compiles_into_a_real_disambiguation_workspace() {
    let (monitor, root, hub, fetch) = setup();
    hub.grant_domain_egress(&monitor, &root, &root, ample_grant(), 1_000)
        .unwrap();
    fetch.register(
        "https://example.com/john-a",
        FetchedPage {
            final_url: None,
            structured: Some(StructuredSignal {
                entity_type: EntityType::Person,
                identifier: None,
                fields: serde_json::json!({ "name": "John Smith Engineer" }),
                relationships: Vec::new(),
            }),
            text: String::new(),
            robots_disallowed: false,
            rate_limited: false,
        },
    );
    fetch.register(
        "https://example.com/john-b",
        FetchedPage {
            final_url: None,
            structured: Some(StructuredSignal {
                entity_type: EntityType::Person,
                identifier: None,
                fields: serde_json::json!({ "name": "John Smith Artist" }),
                relationships: Vec::new(),
            }),
            text: String::new(),
            robots_disallowed: false,
            rate_limited: false,
        },
    );

    hub.web_research(
        &monitor,
        &root,
        &request("https://example.com/john-a"),
        1_000,
    )
    .unwrap();
    let ambiguous = hub
        .web_research(
            &monitor,
            &root,
            &request("https://example.com/john-b"),
            1_001,
        )
        .unwrap();
    assert!(ambiguous.needs_review);

    let compiler = WorkspaceCompiler::new();
    let intent_id = hyperion_storage::ObjectId(1);
    let workspace = present_disambiguation_as_workspace(
        &compiler, &monitor, &root, &ambiguous, intent_id, 1_002,
    )
    .unwrap();

    assert_eq!(workspace.panels.len(), 1);
    let panel = &workspace.panels[0];
    assert_eq!(
        panel.bindings.len(),
        1,
        "the ambiguous object itself must be bound to the review panel"
    );
    assert_eq!(panel.bindings[0].target, ambiguous.object_id);
    assert_eq!(
        panel.accessibility_node.accessible_name,
        "Confirm this is the entity you meant"
    );
}
