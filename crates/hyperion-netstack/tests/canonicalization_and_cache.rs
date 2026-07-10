//! docs/19 §5.1/§5.2: tracking-parameter stripping and case normalization
//! dedupe distinct-looking URLs to the same cache entry / entity, and a
//! live cache hit costs no fetch at all.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_netstack::{
    DomainEgressGrant, FetchedPage, FreshnessPolicy, MockExtractionBackend, MockFetchBackend,
    NetstackHub, StructuredSignal, WebResolutionRequest,
};

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

fn structured_page(entity_type: hyperion_netstack::EntityType, title: &str) -> FetchedPage {
    FetchedPage {
        final_url: None,
        structured: Some(StructuredSignal {
            entity_type,
            identifier: Some("doi:10.1/xyz".to_string()),
            fields: serde_json::json!({ "title": title }),
            relationships: Vec::new(),
        }),
        text: String::new(),
        robots_disallowed: false,
        rate_limited: false,
    }
}

#[test]
fn tracking_parameters_and_casing_dedupe_to_the_same_entity() {
    let (monitor, root, hub, fetch) = setup();
    hub.grant_domain_egress(&monitor, &root, &root, ample_grant(), 1_000)
        .unwrap();
    fetch.register(
        "https://example.com/a",
        structured_page(hyperion_netstack::EntityType::Paper, "A Paper"),
    );

    let first = hub
        .web_research(
            &monitor,
            &root,
            &WebResolutionRequest {
                origin: "HTTPS://Example.com/a?utm_source=twitter".to_string(),
                agent_id: 7,
                purpose: "research".to_string(),
                freshness: FreshnessPolicy::UseCache,
                depth: 0,
            },
            1_000,
        )
        .unwrap();

    let second = hub
        .web_research(
            &monitor,
            &root,
            &WebResolutionRequest {
                origin: "https://example.com/a".to_string(),
                agent_id: 7,
                purpose: "research".to_string(),
                freshness: FreshnessPolicy::UseCache,
                depth: 0,
            },
            1_010,
        )
        .unwrap();

    assert_eq!(first.object_id, second.object_id);
}

#[test]
fn a_live_cache_hit_is_audited_without_a_second_fetch() {
    let (monitor, root, hub, fetch) = setup();
    hub.grant_domain_egress(&monitor, &root, &root, ample_grant(), 1_000)
        .unwrap();
    fetch.register(
        "https://example.com/a",
        structured_page(hyperion_netstack::EntityType::Paper, "A Paper"),
    );

    let request = WebResolutionRequest {
        origin: "https://example.com/a".to_string(),
        agent_id: 7,
        purpose: "research".to_string(),
        freshness: FreshnessPolicy::UseCache,
        depth: 0,
    };
    hub.web_research(&monitor, &root, &request, 1_000).unwrap();
    hub.web_research(&monitor, &root, &request, 1_001).unwrap();

    let cache_hits = hub
        .audit_log()
        .iter()
        .filter(|e| e.kind == "cache_hit")
        .count();
    assert_eq!(cache_hits, 1);
}

#[test]
fn force_revalidate_bypasses_a_live_cache_entry() {
    let (monitor, root, hub, fetch) = setup();
    hub.grant_domain_egress(&monitor, &root, &root, ample_grant(), 1_000)
        .unwrap();
    fetch.register(
        "https://example.com/a",
        structured_page(hyperion_netstack::EntityType::Paper, "A Paper"),
    );

    hub.web_research(
        &monitor,
        &root,
        &WebResolutionRequest {
            origin: "https://example.com/a".to_string(),
            agent_id: 7,
            purpose: "research".to_string(),
            freshness: FreshnessPolicy::UseCache,
            depth: 0,
        },
        1_000,
    )
    .unwrap();

    hub.web_research(
        &monitor,
        &root,
        &WebResolutionRequest {
            origin: "https://example.com/a".to_string(),
            agent_id: 7,
            purpose: "research".to_string(),
            freshness: FreshnessPolicy::ForceRevalidate,
            depth: 0,
        },
        1_001,
    )
    .unwrap();

    let resolved = hub
        .audit_log()
        .iter()
        .filter(|e| e.kind == "resolved")
        .count();
    assert_eq!(resolved, 2);
}

#[test]
fn a_fetch_failure_with_no_prior_cache_falls_back_to_a_stub() {
    let (monitor, root, hub, _fetch) = setup();
    hub.grant_domain_egress(&monitor, &root, &root, ample_grant(), 1_000)
        .unwrap();
    // Nothing registered for this URL, so the mock backend reports NotFound.

    let result = hub
        .web_research(
            &monitor,
            &root,
            &WebResolutionRequest {
                origin: "https://example.com/missing".to_string(),
                agent_id: 7,
                purpose: "research".to_string(),
                freshness: FreshnessPolicy::UseCache,
                depth: 0,
            },
            1_000,
        )
        .unwrap();

    assert!(!result.stale);
    assert!(hub.audit_log().iter().any(|e| e.kind == "stub_fallback"));
}

#[test]
fn a_fetch_failure_after_a_prior_success_falls_back_to_the_stale_cache_entry() {
    let (monitor, root, hub, fetch) = setup();
    hub.grant_domain_egress(&monitor, &root, &root, ample_grant(), 1_000)
        .unwrap();
    fetch.register(
        "https://example.com/a",
        structured_page(hyperion_netstack::EntityType::Paper, "A Paper"),
    );

    let first = hub
        .web_research(
            &monitor,
            &root,
            &WebResolutionRequest {
                origin: "https://example.com/a".to_string(),
                agent_id: 7,
                purpose: "research".to_string(),
                freshness: FreshnessPolicy::UseCache,
                depth: 0,
            },
            1_000,
        )
        .unwrap();

    // The origin now goes down; force revalidation well past the Paper's
    // TTL so the request actually attempts a fetch instead of serving the
    // still-live cache entry.
    fetch.register_error(
        "https://example.com/a",
        hyperion_netstack::FetchError::Timeout("https://example.com/a".to_string()),
    );

    let fallback = hub
        .web_research(
            &monitor,
            &root,
            &WebResolutionRequest {
                origin: "https://example.com/a".to_string(),
                agent_id: 7,
                purpose: "research".to_string(),
                freshness: FreshnessPolicy::UseCache,
                depth: 0,
            },
            1_000 + 40 * 24 * 3600, // well past a Paper's TTL
        )
        .unwrap();

    assert_eq!(first.object_id, fallback.object_id);
    assert!(fallback.stale);
    assert!(hub.audit_log().iter().any(|e| e.kind == "stale_fallback"));
}
