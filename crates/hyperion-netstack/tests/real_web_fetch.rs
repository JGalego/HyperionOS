//! docs/998-roadmap.md M10's real deliverable, proven for real: a real HTTP client fetches
//! a real URL over the real network, a real (non-model) HTML extractor pulls a real entity out of
//! the real response, and `NetstackHub::web_research` merges it into a real Knowledge Graph --
//! not `MockFetchBackend`/`MockExtractionBackend`'s deterministic fixtures.
//!
//! `#[cfg(feature = "real-http")]`-gated like the backends themselves: this test makes real DNS
//! lookups and a real HTTPS request, so it deliberately does not run as part of the default
//! `cargo test --workspace` gate (which must stay network-free and fast) -- invoke explicitly
//! with `cargo test -p hyperion-netstack --features real-http --test real_web_fetch`.

#![cfg(feature = "real-http")]

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_netstack::{
    DomainEgressGrant, FetchBackend, FetchError, FreshnessPolicy, HtmlHeuristicExtractionBackend,
    NetstackHub, ReqwestFetchBackend, WebResolutionRequest,
};

#[test]
fn a_real_fetch_over_the_real_network_merges_a_real_entity_into_the_real_knowledge_graph() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());

    let fetch_backend =
        ReqwestFetchBackend::new().expect("build a real HTTP client with real TLS/DNS");
    let hub = NetstackHub::new(
        graph.clone(),
        Box::new(fetch_backend),
        Box::new(HtmlHeuristicExtractionBackend),
    );

    hub.grant_domain_egress(
        &monitor,
        &root,
        &root,
        DomainEgressGrant {
            domain_patterns: vec!["example.com".to_string()],
            rate_limit_per_window: 10,
            window_secs: 60,
            max_depth: 1,
            expiry: None,
        },
        1_000,
    )
    .expect("grant real domain egress for example.com");

    let request = WebResolutionRequest {
        origin: "https://example.com".to_string(),
        agent_id: 1,
        purpose: "a real M10 integration test".to_string(),
        freshness: FreshnessPolicy::UseCache,
        depth: 0,
    };

    let result = hub
        .web_research(&monitor, &root, &request, 1_001)
        .expect("a real fetch of a real, reachable URL must succeed");

    assert!(
        !result.needs_review,
        "a clean, real page must not be flagged for review"
    );

    let node = graph
        .get(&monitor, &root, result.object_id)
        .expect("the real merged entity must really be readable back from the real graph");
    let title = node.metadata["title"]
        .as_str()
        .expect("a real title field extracted from the real page");
    assert!(
        title.contains("Example Domain"),
        "expected the real page's real <title> content, got: {title:?}"
    );

    let provenance = &node.metadata["_provenance"];
    assert_eq!(
        provenance["extraction_method"], "html-heuristic",
        "must be attributed to the real, non-model HTML extractor, not a mock or a model"
    );
}

#[test]
fn a_real_dns_failure_degrades_to_a_stub_rather_than_panicking() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());

    let fetch_backend = ReqwestFetchBackend::new().unwrap();
    let hub = NetstackHub::new(
        graph.clone(),
        Box::new(fetch_backend),
        Box::new(HtmlHeuristicExtractionBackend),
    );

    let bad_domain = "this-domain-really-does-not-exist-hyperion-m10-test.invalid";
    hub.grant_domain_egress(
        &monitor,
        &root,
        &root,
        DomainEgressGrant {
            domain_patterns: vec![bad_domain.to_string()],
            rate_limit_per_window: 10,
            window_secs: 60,
            max_depth: 1,
            expiry: None,
        },
        1_000,
    )
    .unwrap();

    let request = WebResolutionRequest {
        origin: format!("https://{bad_domain}/"),
        agent_id: 1,
        purpose: "a real M10 DNS-failure test".to_string(),
        freshness: FreshnessPolicy::UseCache,
        depth: 0,
    };

    // docs/19 §10's "stale-but-labeled fallback": with no prior cache entry, a real,
    // unresolvable domain degrades to a real stub node rather than propagating a raw transport
    // error or panicking -- proven here against a real DNS failure, not a hand-fed mock one.
    let result = hub
        .web_research(&monitor, &root, &request, 1_001)
        .expect("a real DNS failure must degrade to a stub, not return an Err");
    assert!(
        !result.stale,
        "no prior cache entry existed to fall back to"
    );

    let node = graph.get(&monitor, &root, result.object_id).unwrap();
    assert_eq!(node.metadata["note"], "network fetch failed");
}

/// docs/19 §13's own "chaos tests" ask, made real: a genuine TLS certificate-validation failure
/// against a real, publicly-reachable host known to serve an expired certificate
/// (`expired.badssl.com`, an intentionally-misconfigured test endpoint badssl.com runs for
/// exactly this purpose) must classify as [`FetchError::Tls`], not silently succeed or surface
/// as [`FetchError::Dns`]/[`FetchError::Timeout`] -- proven directly against
/// [`ReqwestFetchBackend`], not the mock's hand-constructed error value.
#[test]
fn a_real_tls_certificate_failure_classifies_as_tls_not_dns_or_timeout() {
    let backend = ReqwestFetchBackend::new().unwrap();
    let result = backend.fetch("https://expired.badssl.com/");
    assert!(
        matches!(result, Err(FetchError::Tls(_))),
        "expected a real Tls classification, got: {result:?}"
    );
}

/// The other half of docs/19 §13's "chaos tests": a real connection that never completes must
/// classify as [`FetchError::Timeout`].
///
/// The hang is manufactured, not found. Earlier versions of this test looked for somewhere that
/// happened to be slow -- first `httpbin.org/delay/10` (which failed the first time CI ran while
/// that service was returning a real 503 instead of really delaying), then a connection to the
/// closed port `127.0.0.1:1`, on the strength of a one-off probe in one sandbox. Whether a closed
/// port hangs or refuses is a property of the host, not of this crate: a machine that answers with
/// RST -- as a plain Linux container does -- classifies as `ConnectionFailed`, and the test fails
/// for a reason that has nothing to do with the code under test.
///
/// A listener that accepts nothing is deterministic everywhere instead. The kernel completes the
/// TCP handshake from its own backlog, so the connection genuinely establishes; nothing ever reads
/// the request or writes a response, so the client genuinely waits out its own timeout. That is
/// exactly the real-world shape being classified -- a server that took the connection and then
/// went quiet.
#[test]
fn a_real_connection_that_never_completes_classifies_as_timeout() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a real local port");
    let addr = listener.local_addr().unwrap();
    // Deliberately never `accept()`ed, and held open for the whole test so the port stays bound.

    let backend = ReqwestFetchBackend::with_timeout(std::time::Duration::from_secs(2)).unwrap();
    let result = backend.fetch(&format!("http://{addr}/"));
    assert!(
        matches!(result, Err(FetchError::Timeout(_))),
        "expected a real Timeout classification, got: {result:?}"
    );
    drop(listener);
}
