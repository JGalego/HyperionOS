//! docs/27 §3's two-stage separation: capture never writes to the
//! Knowledge Graph; only explicit, consent-gated promotion does — the
//! Phase 9 exit criterion's "without corrupting the Knowledge Graph"
//! guarantee.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_compat::{
    CompatError, CompatHost, CompatibilityProfile, LegacyTarget, NetworkPolicy, PromotionPolicy,
    PromotionState, TrustDepth,
};
use hyperion_knowledge_graph::{GraphQuery, KnowledgeGraph};
use hyperion_netstack::{MockExtractionBackend, MockFetchBackend, NetstackHub};

fn setup() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    CompatHost,
    Arc<KnowledgeGraph>,
) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let netstack = Arc::new(NetstackHub::new(
        graph.clone(),
        Box::new(MockFetchBackend::new()),
        Box::new(MockExtractionBackend),
    ));
    let host = CompatHost::new(graph.clone(), netstack);
    (monitor, root, host, graph)
}

fn linux_profile() -> CompatibilityProfile {
    CompatibilityProfile {
        target: LegacyTarget::Linux,
        min_depth: TrustDepth::D1,
        network_default: NetworkPolicy::Deny,
        filesystem_roots: vec!["/home/guest/Documents".to_string()],
    }
}

#[test]
fn capturing_a_write_never_touches_the_knowledge_graph() {
    let (mut monitor, root, host, graph) = setup();
    let session = host
        .launch(&mut monitor, &root, linux_profile(), TrustDepth::D3, 1_000)
        .unwrap();
    host.grant(&mut monitor, &root, session, RightsMask::WRITE)
        .unwrap();

    host.shim_open(session, "/home/guest/Documents/report.txt", true)
        .unwrap();

    let artifact = host
        .capture_artifact(session, "/home/guest/Documents/report.txt")
        .unwrap();
    assert_eq!(artifact.promotion_state, PromotionState::Pending);

    let hits = graph
        .query(&monitor, &root, &GraphQuery::default())
        .unwrap();
    assert!(
        hits.is_empty(),
        "capture (Stage A) must never write to the Knowledge Graph"
    );
}

#[test]
fn a_path_outside_every_declared_root_is_refused_by_default() {
    let (mut monitor, root, host, _graph) = setup();
    let session = host
        .launch(&mut monitor, &root, linux_profile(), TrustDepth::D3, 1_000)
        .unwrap();
    host.grant(&mut monitor, &root, session, RightsMask::WRITE)
        .unwrap();

    let result = host.shim_open(session, "/etc/passwd", true);
    assert!(matches!(result, Err(CompatError::PathOutsideDeclaredRoots)));
}

#[test]
fn a_write_without_a_prior_grant_is_refused() {
    let (mut monitor, root, host, _graph) = setup();
    let session = host
        .launch(&mut monitor, &root, linux_profile(), TrustDepth::D3, 1_000)
        .unwrap();
    // No `host.grant(...)` call.

    let result = host.shim_open(session, "/home/guest/Documents/report.txt", true);
    assert!(matches!(result, Err(CompatError::WriteNotGranted)));
}

#[test]
fn a_read_within_a_declared_root_needs_no_write_grant() {
    let (mut monitor, root, host, _graph) = setup();
    let session = host
        .launch(&mut monitor, &root, linux_profile(), TrustDepth::D3, 1_000)
        .unwrap();

    assert!(host
        .shim_open(session, "/home/guest/Documents/report.txt", false)
        .is_ok());
}

#[test]
fn explicit_promotion_writes_exactly_one_real_semantic_object() {
    let (mut monitor, root, host, graph) = setup();
    let session = host
        .launch(&mut monitor, &root, linux_profile(), TrustDepth::D3, 1_000)
        .unwrap();
    host.grant(&mut monitor, &root, session, RightsMask::WRITE)
        .unwrap();
    host.shim_open(session, "/home/guest/Documents/report.txt", true)
        .unwrap();

    let object_id = host
        .promote_artifact(
            &monitor,
            &root,
            session,
            "/home/guest/Documents/report.txt",
            PromotionPolicy::AskEveryTime,
            "Document",
            serde_json::json!({"title": "Quarterly Report"}),
            true,
        )
        .unwrap();

    let node = graph.get(&monitor, &root, object_id).unwrap();
    assert_eq!(node.object_type, "Document");

    let artifact = host
        .capture_artifact(session, "/home/guest/Documents/report.txt")
        .unwrap();
    assert_eq!(artifact.promotion_state, PromotionState::Promoted);
    assert_eq!(artifact.promoted_object_id, Some(object_id));
}

#[test]
fn declining_promotion_under_ask_every_time_leaves_the_artifact_ignored_and_the_graph_untouched() {
    let (mut monitor, root, host, graph) = setup();
    let session = host
        .launch(&mut monitor, &root, linux_profile(), TrustDepth::D3, 1_000)
        .unwrap();
    host.grant(&mut monitor, &root, session, RightsMask::WRITE)
        .unwrap();
    host.shim_open(session, "/home/guest/Documents/report.txt", true)
        .unwrap();

    let result = host.promote_artifact(
        &monitor,
        &root,
        session,
        "/home/guest/Documents/report.txt",
        PromotionPolicy::AskEveryTime,
        "Document",
        serde_json::json!({}),
        false,
    );
    assert!(matches!(result, Err(CompatError::PromotionDeclined)));

    let artifact = host
        .capture_artifact(session, "/home/guest/Documents/report.txt")
        .unwrap();
    assert_eq!(artifact.promotion_state, PromotionState::Ignored);

    let hits = graph
        .query(&monitor, &root, &GraphQuery::default())
        .unwrap();
    assert!(hits.is_empty());
}

#[test]
fn a_standing_deny_rule_always_declines_promotion() {
    let (mut monitor, root, host, _graph) = setup();
    let session = host
        .launch(&mut monitor, &root, linux_profile(), TrustDepth::D3, 1_000)
        .unwrap();
    host.grant(&mut monitor, &root, session, RightsMask::WRITE)
        .unwrap();
    host.shim_open(session, "/home/guest/Documents/report.txt", true)
        .unwrap();

    let result = host.promote_artifact(
        &monitor,
        &root,
        session,
        "/home/guest/Documents/report.txt",
        PromotionPolicy::StandingRuleDeny,
        "Document",
        serde_json::json!({}),
        true,
    );
    assert!(
        matches!(result, Err(CompatError::PromotionDeclined)),
        "a standing deny rule must decline even when the caller passes user_confirmed=true"
    );
}

#[test]
fn terminating_a_session_revokes_every_grant_it_was_given() {
    let (mut monitor, root, host, _graph) = setup();
    let session = host
        .launch(&mut monitor, &root, linux_profile(), TrustDepth::D3, 1_000)
        .unwrap();
    host.grant(&mut monitor, &root, session, RightsMask::WRITE)
        .unwrap();
    let session_snapshot = host.session(session).unwrap();
    let granted_token = session_snapshot.grants[0].clone();
    assert!(monitor.check_rights_ok(&granted_token, RightsMask::WRITE));

    host.terminate(&mut monitor, &root, session).unwrap();

    assert!(!monitor.check_rights_ok(&granted_token, RightsMask::WRITE));
    assert!(host.session(session).is_none());
}

#[test]
fn a_target_requiring_deeper_trust_than_available_cannot_launch() {
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

    let windows_profile = CompatibilityProfile {
        target: LegacyTarget::Windows,
        min_depth: TrustDepth::D3,
        network_default: NetworkPolicy::Deny,
        filesystem_roots: vec![],
    };
    let result = host.launch(&mut monitor, &root, windows_profile, TrustDepth::D1, 1_000);
    assert!(matches!(result, Err(CompatError::Unauthorized)));
}
