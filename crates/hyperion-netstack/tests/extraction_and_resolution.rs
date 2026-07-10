//! docs/19 §5.3/§5.4/§5.5: structured signals are preferred over the
//! model-based fallback, entity resolution merges on exact identifier or
//! creates a provisional/new node otherwise, and relationships are
//! written alongside the entity.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{GraphQuery, KnowledgeGraph};
use hyperion_netstack::{
    DomainEgressGrant, EntityType, FetchedPage, FreshnessPolicy, MockExtractionBackend,
    MockFetchBackend, NetstackHub, StructuredSignal, WebResolutionRequest,
};

fn setup() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    NetstackHub,
    Arc<MockFetchBackend>,
    Arc<KnowledgeGraph>,
) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let fetch = Arc::new(MockFetchBackend::new());
    let hub = NetstackHub::new(
        graph.clone(),
        Box::new(fetch.clone()),
        Box::new(MockExtractionBackend),
    );
    (monitor, root, hub, fetch, graph)
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
fn a_structured_signal_is_preferred_over_the_model_fallback() {
    let (monitor, root, hub, fetch, graph) = setup();
    hub.grant_domain_egress(&monitor, &root, &root, ample_grant(), 1_000)
        .unwrap();
    fetch.register(
        "https://example.com/paper",
        FetchedPage {
            final_url: None,
            structured: Some(StructuredSignal {
                entity_type: EntityType::Paper,
                identifier: Some("doi:10.1/abc".to_string()),
                fields: serde_json::json!({ "title": "Transformer Efficiency" }),
                relationships: Vec::new(),
            }),
            text: "some unrelated unstructured fallback text".to_string(),
            robots_disallowed: false,
            rate_limited: false,
        },
    );

    let result = hub
        .web_research(
            &monitor,
            &root,
            &request("https://example.com/paper"),
            1_000,
        )
        .unwrap();
    let node = graph.get(&monitor, &root, result.object_id).unwrap();
    assert_eq!(node.object_type, "Paper");
    assert_eq!(
        node.metadata["title"],
        serde_json::json!("Transformer Efficiency")
    );
    assert_eq!(
        node.metadata["_provenance"]["extraction_method"],
        serde_json::json!("structured-data")
    );
}

#[test]
fn no_structured_signal_falls_back_to_a_generic_web_page() {
    let (monitor, root, hub, fetch, graph) = setup();
    hub.grant_domain_egress(&monitor, &root, &root, ample_grant(), 1_000)
        .unwrap();
    fetch.register(
        "https://example.com/blog",
        FetchedPage {
            final_url: None,
            structured: None,
            text: "My Blog Post Title\nSome body text follows here.".to_string(),
            robots_disallowed: false,
            rate_limited: false,
        },
    );

    let result = hub
        .web_research(&monitor, &root, &request("https://example.com/blog"), 1_000)
        .unwrap();
    let node = graph.get(&monitor, &root, result.object_id).unwrap();
    assert_eq!(node.object_type, "WebPage");
    assert_eq!(
        node.metadata["title"],
        serde_json::json!("My Blog Post Title")
    );
    assert_eq!(
        node.metadata["_provenance"]["extraction_method"],
        serde_json::json!("model-based")
    );
}

#[test]
fn an_exact_identifier_match_merges_into_the_existing_node() {
    let (monitor, root, hub, fetch, graph) = setup();
    hub.grant_domain_egress(&monitor, &root, &root, ample_grant(), 1_000)
        .unwrap();
    fetch.register(
        "https://example.com/v1",
        FetchedPage {
            final_url: None,
            structured: Some(StructuredSignal {
                entity_type: EntityType::Paper,
                identifier: Some("doi:10.1/abc".to_string()),
                fields: serde_json::json!({ "title": "Transformer Efficiency" }),
                relationships: Vec::new(),
            }),
            text: String::new(),
            robots_disallowed: false,
            rate_limited: false,
        },
    );
    fetch.register(
        "https://example.com/v2",
        FetchedPage {
            final_url: None,
            structured: Some(StructuredSignal {
                entity_type: EntityType::Paper,
                identifier: Some("doi:10.1/abc".to_string()),
                fields: serde_json::json!({ "title": "Transformer Efficiency (mirror)" }),
                relationships: Vec::new(),
            }),
            text: String::new(),
            robots_disallowed: false,
            rate_limited: false,
        },
    );

    let first = hub
        .web_research(&monitor, &root, &request("https://example.com/v1"), 1_000)
        .unwrap();
    let second = hub
        .web_research(&monitor, &root, &request("https://example.com/v2"), 1_001)
        .unwrap();

    assert_eq!(first.object_id, second.object_id);
    assert!(!second.needs_review);

    let hits = graph
        .query(
            &monitor,
            &root,
            &GraphQuery {
                type_filter: Some(vec!["Paper".to_string()]),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        hits.len(),
        1,
        "the second fetch must merge, not create a sibling node"
    );
}

#[test]
fn a_near_duplicate_title_with_no_identifier_is_provisional_not_silently_merged() {
    let (monitor, root, hub, fetch, graph) = setup();
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

    let first = hub
        .web_research(
            &monitor,
            &root,
            &request("https://example.com/john-a"),
            1_000,
        )
        .unwrap();
    let second = hub
        .web_research(
            &monitor,
            &root,
            &request("https://example.com/john-b"),
            1_001,
        )
        .unwrap();

    assert_ne!(
        first.object_id, second.object_id,
        "an ambiguous match must never be silently merged"
    );
    assert!(second.needs_review);

    let hits = graph
        .query(
            &monitor,
            &root,
            &GraphQuery {
                type_filter: Some(vec!["Person".to_string()]),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(hits.len(), 2);
}

#[test]
fn unrelated_entities_of_the_same_type_are_created_distinctly() {
    let (monitor, root, hub, fetch, _graph) = setup();
    hub.grant_domain_egress(&monitor, &root, &root, ample_grant(), 1_000)
        .unwrap();
    fetch.register(
        "https://example.com/acme",
        FetchedPage {
            final_url: None,
            structured: Some(StructuredSignal {
                entity_type: EntityType::Organization,
                identifier: None,
                fields: serde_json::json!({ "name": "Acme Corp" }),
                relationships: Vec::new(),
            }),
            text: String::new(),
            robots_disallowed: false,
            rate_limited: false,
        },
    );
    fetch.register(
        "https://example.com/globex",
        FetchedPage {
            final_url: None,
            structured: Some(StructuredSignal {
                entity_type: EntityType::Organization,
                identifier: None,
                fields: serde_json::json!({ "name": "Globex Industries" }),
                relationships: Vec::new(),
            }),
            text: String::new(),
            robots_disallowed: false,
            rate_limited: false,
        },
    );

    let first = hub
        .web_research(&monitor, &root, &request("https://example.com/acme"), 1_000)
        .unwrap();
    let second = hub
        .web_research(
            &monitor,
            &root,
            &request("https://example.com/globex"),
            1_001,
        )
        .unwrap();

    assert_ne!(first.object_id, second.object_id);
    assert!(!second.needs_review);
}

#[test]
fn relationships_are_written_alongside_the_entity() {
    let (monitor, root, hub, fetch, graph) = setup();
    hub.grant_domain_egress(&monitor, &root, &root, ample_grant(), 1_000)
        .unwrap();
    fetch.register(
        "https://example.com/paper-with-author",
        FetchedPage {
            final_url: None,
            structured: Some(StructuredSignal {
                entity_type: EntityType::Paper,
                identifier: Some("doi:10.1/xyz".to_string()),
                fields: serde_json::json!({ "title": "A Paper" }),
                relationships: vec![("authored_by".to_string(), "orcid:0000-0001".to_string())],
            }),
            text: String::new(),
            robots_disallowed: false,
            rate_limited: false,
        },
    );

    let result = hub
        .web_research(
            &monitor,
            &root,
            &request("https://example.com/paper-with-author"),
            1_000,
        )
        .unwrap();

    let author_hits = graph
        .query(
            &monitor,
            &root,
            &GraphQuery {
                type_filter: Some(vec!["Person".to_string()]),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(author_hits.len(), 1);
    assert_eq!(
        author_hits[0].node.metadata["identifier"],
        serde_json::json!("orcid:0000-0001")
    );

    let subgraph = graph
        .traverse(&monitor, &root, result.object_id, None, 1)
        .unwrap();
    assert!(subgraph
        .edges
        .iter()
        .any(|(_, e)| e.predicate == "authored_by" && e.subject == result.object_id));
}
