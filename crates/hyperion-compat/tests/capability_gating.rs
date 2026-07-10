//! Mirrors every other crate in this workspace: every call is capability-
//! gated, re-checked live against the monitor.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_compat::{
    CompatError, CompatHost, CompatibilityProfile, LegacyTarget, NetworkPolicy, TrustDepth,
};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_netstack::{MockExtractionBackend, MockFetchBackend, NetstackHub};

fn linux_profile() -> CompatibilityProfile {
    CompatibilityProfile {
        target: LegacyTarget::Linux,
        min_depth: TrustDepth::D1,
        network_default: NetworkPolicy::Deny,
        filesystem_roots: vec!["/home/guest".to_string()],
    }
}

fn host_and_graph() -> (CompatHost, Arc<KnowledgeGraph>) {
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let netstack = Arc::new(NetstackHub::new(
        graph.clone(),
        Box::new(MockFetchBackend::new()),
        Box::new(MockExtractionBackend),
    ));
    (CompatHost::new(graph.clone(), netstack), graph)
}

#[test]
fn launch_requires_grant_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let no_grant = monitor
        .cap_derive(
            &root,
            RightsMask::READ | RightsMask::WRITE,
            None,
            TrustBoundaryId(2),
        )
        .unwrap();
    let (host, _graph) = host_and_graph();

    let result = host.launch(
        &mut monitor,
        &no_grant,
        linux_profile(),
        TrustDepth::D3,
        1_000,
    );
    assert!(matches!(result, Err(CompatError::Unauthorized)));
}

#[test]
fn terminate_requires_revoke_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let (host, _graph) = host_and_graph();
    let session = host
        .launch(&mut monitor, &root, linux_profile(), TrustDepth::D3, 1_000)
        .unwrap();

    let no_revoke = monitor
        .cap_derive(
            &root,
            RightsMask::READ | RightsMask::WRITE | RightsMask::GRANT,
            None,
            TrustBoundaryId(2),
        )
        .unwrap();
    let result = host.terminate(&mut monitor, &no_revoke, session);
    assert!(matches!(result, Err(CompatError::Unauthorized)));
}

#[test]
fn revoking_the_admin_token_blocks_further_launches_re_checked_live() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let delegate = monitor
        .cap_derive(&root, RightsMask::all(), None, TrustBoundaryId(2))
        .unwrap();
    let (host, _graph) = host_and_graph();

    assert!(host
        .launch(
            &mut monitor,
            &delegate,
            linux_profile(),
            TrustDepth::D3,
            1_000
        )
        .is_ok());

    monitor.cap_revoke(&delegate);

    assert!(matches!(
        host.launch(
            &mut monitor,
            &delegate,
            linux_profile(),
            TrustDepth::D3,
            1_001
        ),
        Err(CompatError::Unauthorized)
    ));
}
