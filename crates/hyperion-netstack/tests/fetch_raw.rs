//! docs/19 §3.1/§6's `web.fetch.raw`: the same authorization/SSRF gate as
//! `web.research`, but no Knowledge Graph merge.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{GraphQuery, KnowledgeGraph};
use hyperion_netstack::{
    DomainEgressGrant, FetchedPage, MockExtractionBackend, MockFetchBackend, NetstackError,
    NetstackHub,
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
        domain_patterns: vec!["bank.example.com".to_string()],
        rate_limit_per_window: 100,
        window_secs: 60,
        max_depth: 5,
        expiry: None,
    }
}

#[test]
fn fetch_raw_returns_the_backend_payload_verbatim_with_no_graph_merge() {
    let (monitor, root, hub, fetch, graph) = setup();
    hub.grant_domain_egress(&monitor, &root, &root, ample_grant(), 1_000)
        .unwrap();
    fetch.register(
        "https://bank.example.com/portal",
        FetchedPage {
            final_url: None,
            structured: None,
            text: "<html>legacy portal DOM</html>".to_string(),
            robots_disallowed: false,
            rate_limited: false,
        },
    );

    let page = hub
        .web_fetch_raw(&monitor, &root, "https://bank.example.com/portal", 7, 1_000)
        .unwrap();
    assert_eq!(page.text, "<html>legacy portal DOM</html>");

    let hits = graph
        .query(&monitor, &root, &GraphQuery::default())
        .unwrap();
    assert!(
        hits.is_empty(),
        "web.fetch.raw must never merge into the Knowledge Graph"
    );
}

#[test]
fn fetch_raw_still_enforces_ssrf_containment() {
    let (monitor, root, hub, _fetch, _graph) = setup();
    hub.grant_domain_egress(
        &monitor,
        &root,
        &root,
        DomainEgressGrant {
            domain_patterns: vec!["127.0.0.1".to_string()],
            rate_limit_per_window: 100,
            window_secs: 60,
            max_depth: 5,
            expiry: None,
        },
        1_000,
    )
    .unwrap();

    let result = hub.web_fetch_raw(&monitor, &root, "http://127.0.0.1/admin", 7, 1_000);
    assert!(matches!(result, Err(NetstackError::SsrfBlocked(_))));
}

#[test]
fn fetch_raw_still_requires_a_domain_egress_grant() {
    let (monitor, root, hub, _fetch, _graph) = setup();
    let result = hub.web_fetch_raw(&monitor, &root, "https://bank.example.com/portal", 7, 1_000);
    assert!(matches!(result, Err(NetstackError::NoGrant)));
}
