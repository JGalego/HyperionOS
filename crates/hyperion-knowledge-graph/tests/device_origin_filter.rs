//! This crate's own previously-named "device_origin-based filtering (a finer axis than plain
//! owner)" gap: two devices under the *same* owner can narrow a [`GraphQuery`]/
//! [`KnowledgeGraph::traverse_with_device_origin_filter`] call to just what one of them recorded,
//! without that narrowing ever widening visibility across a Trust Boundary it wouldn't otherwise
//! see (docs/29's own worked example: `joao-phone-uuid`/`joao-laptop-uuid` share one `owner_id`).

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{EdgeOrigin, GraphQuery, KnowledgeGraph};
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

const PHONE: u64 = 1;
const LAPTOP: u64 = 2;

#[test]
fn a_plain_put_node_records_no_device_origin() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let id = graph
        .put_node(&monitor, &token, None, "note", None, json!({}))
        .unwrap();
    let node = graph.get(&monitor, &token, id).unwrap();
    assert_eq!(
        node.device_origin, 0,
        "a caller with no real device identity to supply gets an honest 0, not a guess"
    );
}

#[test]
fn put_node_with_device_origin_records_it_and_owner_still_reflects_the_callers_boundary() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let id = graph
        .put_node_with_device_origin(&monitor, &token, None, "note", None, json!({}), PHONE)
        .unwrap();
    let node = graph.get(&monitor, &token, id).unwrap();
    assert_eq!(node.device_origin, PHONE);
    assert_eq!(
        node.owner, 1,
        "owner is the caller's own Trust Boundary, unaffected by device_origin"
    );
}

#[test]
fn updating_an_existing_node_overwrites_device_origin_to_whichever_device_wrote_this_version() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let id = graph
        .put_node_with_device_origin(&monitor, &token, None, "note", None, json!({"v": 1}), PHONE)
        .unwrap();
    graph
        .put_node_with_device_origin(
            &monitor,
            &token,
            Some(id),
            "note",
            None,
            json!({"v": 2}),
            LAPTOP,
        )
        .unwrap();

    let node = graph.get(&monitor, &token, id).unwrap();
    assert_eq!(
        node.device_origin, LAPTOP,
        "device_origin names whoever authored the CURRENT version, unlike owner which is \
         preserved verbatim across an update"
    );
}

#[test]
fn query_device_origin_filter_narrows_within_one_owner_not_across_it() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let from_phone = graph
        .put_node_with_device_origin(
            &monitor,
            &token,
            None,
            "photo",
            None,
            json!({"taken_on": "phone"}),
            PHONE,
        )
        .unwrap();
    let from_laptop = graph
        .put_node_with_device_origin(
            &monitor,
            &token,
            None,
            "photo",
            None,
            json!({"taken_on": "laptop"}),
            LAPTOP,
        )
        .unwrap();

    let phone_only = GraphQuery {
        device_origin_filter: Some(PHONE),
        limit: 0,
        ..Default::default()
    };
    let hits = graph.query(&monitor, &token, &phone_only).unwrap();
    let ids: Vec<_> = hits.iter().map(|h| h.node_id).collect();
    assert_eq!(ids, vec![from_phone]);
    assert!(!ids.contains(&from_laptop));

    let no_filter = GraphQuery {
        limit: 0,
        ..Default::default()
    };
    let all_hits = graph.query(&monitor, &token, &no_filter).unwrap();
    assert_eq!(
        all_hits.len(),
        2,
        "with no device_origin_filter, both of this owner's own devices' nodes still show up"
    );
}

#[test]
fn query_device_origin_filter_never_reveals_a_different_owners_node() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let mine = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let theirs = monitor.mint_root(RightsMask::all(), TrustBoundaryId(2), None);
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    graph
        .put_node_with_device_origin(&monitor, &theirs, None, "note", None, json!({}), PHONE)
        .unwrap();

    let query = GraphQuery {
        device_origin_filter: Some(PHONE),
        limit: 0,
        ..Default::default()
    };
    let hits = graph.query(&monitor, &mine, &query).unwrap();
    assert!(
        hits.is_empty(),
        "a device_origin_filter narrows within the caller's own owner boundary; it must never \
         surface a different Trust Boundary's node even if its device_origin matches"
    );
}

#[test]
fn explain_ranking_reports_device_origin_matched() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let from_phone = graph
        .put_node_with_device_origin(&monitor, &token, None, "note", None, json!({}), PHONE)
        .unwrap();

    let matching = GraphQuery {
        device_origin_filter: Some(PHONE),
        ..Default::default()
    };
    let rationale = graph
        .explain_ranking(&monitor, &token, from_phone, &matching)
        .unwrap();
    assert_eq!(rationale.device_origin_matched, Some(true));
    assert!(rationale.would_be_included);

    let non_matching = GraphQuery {
        device_origin_filter: Some(LAPTOP),
        ..Default::default()
    };
    let rationale = graph
        .explain_ranking(&monitor, &token, from_phone, &non_matching)
        .unwrap();
    assert_eq!(rationale.device_origin_matched, Some(false));
    assert!(!rationale.would_be_included);

    let no_filter = GraphQuery::default();
    let rationale = graph
        .explain_ranking(&monitor, &token, from_phone, &no_filter)
        .unwrap();
    assert_eq!(rationale.device_origin_matched, None);
}

#[test]
fn traverse_with_device_origin_filter_excludes_a_neighbor_from_a_different_device() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

    let anchor = graph
        .put_node_with_device_origin(&monitor, &token, None, "trip", None, json!({}), PHONE)
        .unwrap();
    let phone_photo = graph
        .put_node_with_device_origin(&monitor, &token, None, "photo", None, json!({}), PHONE)
        .unwrap();
    let laptop_doc = graph
        .put_node_with_device_origin(&monitor, &token, None, "document", None, json!({}), LAPTOP)
        .unwrap();

    graph
        .link(
            &monitor,
            &token,
            anchor,
            "part_of",
            phone_photo,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "test",
            None,
        )
        .unwrap();
    graph
        .link(
            &monitor,
            &token,
            anchor,
            "part_of",
            laptop_doc,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "test",
            None,
        )
        .unwrap();

    let subgraph = graph
        .traverse_with_device_origin_filter(&monitor, &token, anchor, None, 1, Some(PHONE))
        .unwrap();
    let ids: Vec<_> = subgraph.nodes.iter().map(|(id, _, _)| *id).collect();
    assert!(ids.contains(&phone_photo));
    assert!(
        !ids.contains(&laptop_doc),
        "a neighbor authored by a different device must be excluded from expansion entirely"
    );

    let unfiltered = graph.traverse(&monitor, &token, anchor, None, 1).unwrap();
    let unfiltered_ids: Vec<_> = unfiltered.nodes.iter().map(|(id, _, _)| *id).collect();
    assert!(
        unfiltered_ids.contains(&laptop_doc),
        "plain traverse (no device_origin_filter) still reaches both devices' own nodes"
    );
}
