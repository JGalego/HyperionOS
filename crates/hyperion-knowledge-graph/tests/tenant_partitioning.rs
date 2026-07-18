//! This crate's own previously-named "KG partitioning / `TenantPartition` / cross-tenant edges...
//! no partitioning logic exists here" gap: `NodeRecord::tenant_id` is docs/37 §Data Structures'
//! real `TenantPartition.tenant_id`, and `KnowledgeGraph::link`'s own real cross-tenant gate is
//! docs/37 §Algorithms 3's "no default-open cross-partition read" -- linking two nodes recorded
//! under different tenants requires the caller's token to also carry `RightsMask::GRANT`.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{EdgeOrigin, GraphError, KnowledgeGraph, TenantId};
use serde_json::json;

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    (dir, monitor, token)
}

const TENANT_A: TenantId = TenantId(1);
const TENANT_B: TenantId = TenantId(2);

#[test]
fn a_plain_put_node_defaults_to_no_tenant_recorded() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let id = graph
        .put_node(&monitor, &token, None, "note", None, json!({}))
        .unwrap();
    let node = graph.get(&monitor, &token, id).unwrap();
    assert_eq!(
        node.tenant_id,
        TenantId::default(),
        "a caller with no real tenant to supply must get the honest single-tenant default"
    );
}

#[test]
fn put_node_with_tenant_records_a_real_tenant_id() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let id = graph
        .put_node_with_tenant(&monitor, &token, None, "note", None, json!({}), TENANT_A)
        .unwrap();
    let node = graph.get(&monitor, &token, id).unwrap();
    assert_eq!(node.tenant_id, TENANT_A);
}

#[test]
fn linking_two_nodes_in_the_same_tenant_needs_no_special_grant() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let a = graph
        .put_node_with_tenant(&monitor, &token, None, "note", None, json!({}), TENANT_A)
        .unwrap();
    let b = graph
        .put_node_with_tenant(&monitor, &token, None, "note", None, json!({}), TENANT_A)
        .unwrap();

    let result = graph.link(
        &monitor,
        &token,
        a,
        "relates_to",
        b,
        1.0,
        EdgeOrigin::Explicit,
        None,
        "test",
        None,
    );
    assert!(result.is_ok());
}

#[test]
fn linking_across_tenants_without_grant_rights_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let a = graph
        .put_node_with_tenant(&monitor, &root, None, "note", None, json!({}), TENANT_A)
        .unwrap();
    let b = graph
        .put_node_with_tenant(&monitor, &root, None, "note", None, json!({}), TENANT_B)
        .unwrap();

    // A token with WRITE but not GRANT.
    let write_only = monitor
        .cap_derive(&root, RightsMask::WRITE, None, TrustBoundaryId(1))
        .unwrap();

    let result = graph.link(
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
        result,
        Err(GraphError::CrossTenantGrantRequired(TENANT_A, TENANT_B))
    ));
}

#[test]
fn linking_across_tenants_with_grant_rights_succeeds() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let a = graph
        .put_node_with_tenant(&monitor, &token, None, "note", None, json!({}), TENANT_A)
        .unwrap();
    let b = graph
        .put_node_with_tenant(&monitor, &token, None, "note", None, json!({}), TENANT_B)
        .unwrap();

    // `token` (from `setup()`) carries `RightsMask::all()`, which includes GRANT.
    let result = graph.link(
        &monitor,
        &token,
        a,
        "relates_to",
        b,
        1.0,
        EdgeOrigin::Explicit,
        None,
        "test",
        None,
    );
    assert!(result.is_ok());
}

#[test]
fn a_single_tenant_deployment_is_never_affected_by_the_cross_tenant_gate() {
    // Every existing caller in this workspace uses plain `put_node` (tenant_id defaulting to
    // `TenantId(0)` on both sides) -- the gate must never trip for them, even with only WRITE
    // rights, since `0 == 0` never counts as cross-tenant.
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let a = graph
        .put_node(&monitor, &root, None, "note", None, json!({}))
        .unwrap();
    let b = graph
        .put_node(&monitor, &root, None, "note", None, json!({}))
        .unwrap();
    let write_only = monitor
        .cap_derive(&root, RightsMask::WRITE, None, TrustBoundaryId(1))
        .unwrap();

    let result = graph.link(
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
    assert!(result.is_ok());
}
