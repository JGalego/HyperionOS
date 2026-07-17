//! docs/19 §6: "Event `web.entity.resolved`, published on the Event
//! System, lets other Agents/Workspaces react to newly resolved entities
//! without polling." Proves `NetstackHub::new_with_events` actually
//! publishes it, and that a hub built via the plain `NetstackHub::new`
//! (every other test in this crate) publishes nothing.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_events::{BackpressurePolicy, DeliveryClass, EventBus, TopicKind, TopicPattern};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_netstack::{
    DomainEgressGrant, FetchedPage, FreshnessPolicy, MockExtractionBackend, MockFetchBackend,
    NetstackHub, StructuredSignal, WebResolutionRequest,
};

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

fn register_page(fetch: &MockFetchBackend, url: &str) {
    fetch.register(
        url,
        FetchedPage {
            final_url: None,
            structured: Some(StructuredSignal {
                entity_type: hyperion_netstack::EntityType::Paper,
                identifier: Some("doi:10.1/abc".to_string()),
                fields: serde_json::json!({ "title": "Transformer Efficiency" }),
                relationships: Vec::new(),
            }),
            text: "some unrelated unstructured fallback text".to_string(),
            robots_disallowed: false,
            rate_limited: false,
        },
    );
}

#[test]
fn web_research_publishes_a_real_web_entity_resolved_event() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let fetch = Arc::new(MockFetchBackend::new());
    let bus = Arc::new(EventBus::new(None));

    let hub = NetstackHub::new_with_events(
        graph,
        Box::new(fetch.clone()),
        Box::new(MockExtractionBackend),
        bus.clone(),
    );
    hub.grant_domain_egress(&monitor, &root, &root, ample_grant(), 1_000)
        .unwrap();

    // A dashboard-style observer with no prior knowledge of which node id
    // will be created subscribes kind-wide, the same real use case
    // docs/34's audit sink has -- needs GRANT, per this bus's own
    // KindScoped authorization rule.
    let observer = monitor.mint_root(
        RightsMask::READ | RightsMask::GRANT,
        TrustBoundaryId(1),
        None,
    );
    let sub = bus
        .subscribe(
            &monitor,
            &observer,
            TrustBoundaryId(1),
            TopicPattern::KindScoped(TopicKind::ObjectChanged),
            DeliveryClass::AtMostOnce,
            BackpressurePolicy::Buffer { capacity: 8 },
        )
        .unwrap();

    register_page(&fetch, "https://example.com/paper");
    let result = hub
        .web_research(
            &monitor,
            &root,
            &request("https://example.com/paper"),
            1_000,
        )
        .unwrap();

    let event = sub
        .try_recv()
        .expect("web_research should have published a web.entity.resolved event");
    assert_eq!(event.topic.schema_id.0, "web.entity.resolved");
    assert_eq!(
        event.payload,
        hyperion_events::EventPayload::Inline(serde_json::json!({
            "object_id": result.object_id.0,
            "needs_review": false,
            "resolved_at": 1_000,
        }))
    );
    assert!(sub.try_recv().is_none());
}

#[test]
fn a_hub_without_a_wired_bus_publishes_nothing_but_still_resolves() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let fetch = Arc::new(MockFetchBackend::new());

    // Plain `new`, no events wired -- every other test in this crate uses
    // this constructor and must keep working unchanged.
    let hub = NetstackHub::new(
        graph,
        Box::new(fetch.clone()),
        Box::new(MockExtractionBackend),
    );
    hub.grant_domain_egress(&monitor, &root, &root, ample_grant(), 1_000)
        .unwrap();
    register_page(&fetch, "https://example.com/paper");

    let result = hub
        .web_research(
            &monitor,
            &root,
            &request("https://example.com/paper"),
            1_000,
        )
        .unwrap();
    assert!(
        !result.needs_review,
        "resolution itself is unaffected by whether a bus is wired"
    );
}
