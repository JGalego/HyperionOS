//! Real, cross-crate proof that `hyperion_scalability::tenant_grant_cross_partition` mints
//! exactly the capability `hyperion_knowledge_graph::KnowledgeGraph::link`'s own real cross-tenant
//! gate checks for -- not two independently-tested pieces that happen to share a name.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{EdgeOrigin, GraphError, KnowledgeGraph, TenantId};
use hyperion_scalability::tenant_grant_cross_partition;
use serde_json::json;

#[test]
fn a_real_cross_partition_grant_lets_the_kg_actually_link_across_tenants() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let a = graph
        .put_node_with_tenant(&monitor, &root, None, "note", None, json!({}), TenantId(1))
        .unwrap();
    let b = graph
        .put_node_with_tenant(&monitor, &root, None, "note", None, json!({}), TenantId(2))
        .unwrap();

    // A caller with only WRITE (no GRANT) is refused, matching the real cross-tenant gate.
    let write_only = monitor
        .cap_derive(&root, RightsMask::WRITE, None, TrustBoundaryId(1))
        .unwrap();
    let refused = graph.link(
        &monitor,
        &write_only,
        a,
        "relates_to",
        b,
        1.0,
        EdgeOrigin::Explicit,
        None,
        "test",
        None,
    );
    assert!(matches!(
        refused,
        Err(GraphError::CrossTenantGrantRequired(_, _))
    ));

    // The real grant this crate mints must actually unblock it.
    let grant = tenant_grant_cross_partition(&mut monitor, &root, TrustBoundaryId(1)).unwrap();
    let allowed = graph.link(
        &monitor,
        &grant,
        a,
        "relates_to",
        b,
        1.0,
        EdgeOrigin::Explicit,
        None,
        "test",
        None,
    );
    assert!(
        allowed.is_ok(),
        "a real hyperion_scalability::tenant_grant_cross_partition grant must satisfy \
         hyperion_knowledge_graph::KnowledgeGraph::link's own real cross-tenant gate, got: \
         {allowed:?}"
    );
}
