//! Mirrors every other crate in this workspace: every call is capability-
//! gated, re-checked live against the monitor.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_netstack::{
    DomainEgressGrant, FreshnessPolicy, MockExtractionBackend, MockFetchBackend, NetstackError,
    NetstackHub, WebResolutionRequest,
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

#[test]
fn grant_domain_egress_requires_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let hub = NetstackHub::new(
        graph,
        Box::new(MockFetchBackend::new()),
        Box::new(MockExtractionBackend),
    );

    let result = hub.grant_domain_egress(&monitor, &read_only, &read_only, ample_grant(), 1_000);
    assert!(matches!(result, Err(NetstackError::Unauthorized)));
}

#[test]
fn web_research_requires_exec_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let write_only = monitor
        .cap_derive(&root, RightsMask::WRITE, None, TrustBoundaryId(2))
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let hub = NetstackHub::new(
        graph,
        Box::new(MockFetchBackend::new()),
        Box::new(MockExtractionBackend),
    );
    hub.grant_domain_egress(&monitor, &root, &write_only, ample_grant(), 1_000)
        .unwrap();

    let result = hub.web_research(
        &monitor,
        &write_only,
        &WebResolutionRequest {
            origin: "https://example.com/a".to_string(),
            agent_id: 1,
            purpose: "research".to_string(),
            freshness: FreshnessPolicy::UseCache,
            depth: 0,
        },
        1_000,
    );
    assert!(matches!(result, Err(NetstackError::Unauthorized)));
}

#[test]
fn revoking_a_token_blocks_further_access_re_checked_live() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let delegate = monitor
        .cap_derive(&root, RightsMask::all(), None, TrustBoundaryId(2))
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let hub = NetstackHub::new(
        graph,
        Box::new(MockFetchBackend::new()),
        Box::new(MockExtractionBackend),
    );

    assert!(hub
        .grant_domain_egress(&monitor, &delegate, &delegate, ample_grant(), 1_000)
        .is_ok());

    monitor.cap_revoke(&delegate);

    assert!(matches!(
        hub.grant_domain_egress(&monitor, &delegate, &delegate, ample_grant(), 1_001),
        Err(NetstackError::Unauthorized)
    ));
}
