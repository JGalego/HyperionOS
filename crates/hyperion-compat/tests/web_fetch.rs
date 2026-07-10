//! docs/27 §5: for a Web-target session, `NetworkPolicy::Allow(scope)`
//! resolves at admission time into a real `web.fetch.raw` grant scoped
//! to that domain — not a second, unrelated network-access path.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_compat::{
    CompatError, CompatHost, CompatibilityProfile, LegacyTarget, NetworkPolicy, TrustDepth,
};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_netstack::{FetchedPage, MockExtractionBackend, MockFetchBackend, NetstackHub};

#[test]
fn a_web_session_with_an_allowed_domain_can_fetch_within_that_scope() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let fetch_backend = Arc::new(MockFetchBackend::new());
    fetch_backend.register(
        "https://legacy-bank.example/portal",
        FetchedPage {
            final_url: None,
            structured: None,
            text: "<html>legacy portal</html>".to_string(),
            robots_disallowed: false,
            rate_limited: false,
        },
    );
    let netstack = Arc::new(NetstackHub::new(
        graph.clone(),
        Box::new(fetch_backend.clone()),
        Box::new(MockExtractionBackend),
    ));
    let host = CompatHost::new(graph, netstack);

    let profile = CompatibilityProfile {
        target: LegacyTarget::Web,
        min_depth: TrustDepth::D0,
        network_default: NetworkPolicy::Allow {
            scope: "legacy-bank.example".to_string(),
        },
        filesystem_roots: vec![],
    };
    let session = host
        .launch(&mut monitor, &root, profile, TrustDepth::D1, 1_000)
        .unwrap();

    let page = host
        .web_fetch(
            &monitor,
            &root,
            session,
            "https://legacy-bank.example/portal",
            7,
            1_000,
        )
        .unwrap();
    assert_eq!(page.text, "<html>legacy portal</html>");
}

#[test]
fn a_non_web_session_cannot_use_the_web_fetch_path_at_all() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let netstack = Arc::new(NetstackHub::new(
        graph.clone(),
        Box::new(MockFetchBackend::new()),
        Box::new(MockExtractionBackend),
    ));
    let host = CompatHost::new(graph, netstack);

    let profile = CompatibilityProfile {
        target: LegacyTarget::Linux,
        min_depth: TrustDepth::D1,
        network_default: NetworkPolicy::Allow {
            scope: "example.com".to_string(),
        },
        filesystem_roots: vec![],
    };
    let session = host
        .launch(&mut monitor, &root, profile, TrustDepth::D2, 1_000)
        .unwrap();

    let result = host.web_fetch(&monitor, &root, session, "https://example.com/a", 7, 1_000);
    assert!(matches!(result, Err(CompatError::NotAnAllowedWebSession)));
}

#[test]
fn a_web_session_with_network_denied_cannot_fetch() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let netstack = Arc::new(NetstackHub::new(
        graph.clone(),
        Box::new(MockFetchBackend::new()),
        Box::new(MockExtractionBackend),
    ));
    let host = CompatHost::new(graph, netstack);

    let profile = CompatibilityProfile {
        target: LegacyTarget::Web,
        min_depth: TrustDepth::D0,
        network_default: NetworkPolicy::Deny,
        filesystem_roots: vec![],
    };
    let session = host
        .launch(&mut monitor, &root, profile, TrustDepth::D1, 1_000)
        .unwrap();

    let result = host.web_fetch(&monitor, &root, session, "https://example.com/a", 7, 1_000);
    assert!(matches!(result, Err(CompatError::NotAnAllowedWebSession)));
}
