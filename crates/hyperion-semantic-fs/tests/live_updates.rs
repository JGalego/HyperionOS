//! docs/10-semantic-filesystem.md §Performance Analysis: "VirtualFolders are
//! cached with a TTL and invalidated incrementally: rather than re-running a
//! full traversal on every access, the Query Resolver subscribes to relevant
//! object/edge changes via the Event System and only re-materializes the
//! specific folders whose inputs actually changed."

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_events::EventBus;
use hyperion_knowledge_graph::{EdgeOrigin, KnowledgeGraph};
use hyperion_semantic_fs::{QuerySpec, SemanticFilesystem};
use serde_json::json;

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    Arc<KnowledgeGraph>,
    SemanticFilesystem,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let bus = Arc::new(EventBus::new(None));
    let graph = Arc::new(
        KnowledgeGraph::open(dir.path().join("kg.jsonl"))
            .unwrap()
            .with_events(bus.clone()),
    );
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let fs = SemanticFilesystem::new(graph.clone(), context)
        .with_events(&monitor, &token, bus)
        .unwrap();
    (dir, monitor, token, graph, fs)
}

#[test]
fn a_repeat_query_of_the_same_shape_is_a_real_cache_hit() {
    let (_dir, monitor, token, graph, fs) = setup();
    let trip = graph
        .put_node(
            &monitor,
            &token,
            None,
            "trip",
            None,
            json!({"title": "Hawaii"}),
        )
        .unwrap();
    let spec = QuerySpec {
        anchor: Some(trip),
        hop_bound: 1,
        ttl_secs: 300,
        ..Default::default()
    };

    let first = fs.query(&monitor, &token, &spec).unwrap();
    let second = fs.query(&monitor, &token, &spec).unwrap();
    assert_eq!(
        first.folder_id, second.folder_id,
        "same query shape, nothing changed -- must reuse the cached folder"
    );
}

#[test]
fn a_write_touching_the_anchor_invalidates_the_cached_folder() {
    let (_dir, monitor, token, graph, fs) = setup();
    let trip = graph
        .put_node(
            &monitor,
            &token,
            None,
            "trip",
            None,
            json!({"title": "Hawaii"}),
        )
        .unwrap();
    let spec = QuerySpec {
        anchor: Some(trip),
        hop_bound: 1,
        ttl_secs: 300,
        ..Default::default()
    };

    let first = fs.query(&monitor, &token, &spec).unwrap();

    let photo = graph
        .put_node(
            &monitor,
            &token,
            None,
            "photo",
            None,
            json!({"title": "new"}),
        )
        .unwrap();
    graph
        .link(
            &monitor,
            &token,
            photo,
            "part_of_trip",
            trip,
            1.0,
            EdgeOrigin::Inferred,
            None,
            "agent",
            None,
        )
        .unwrap();

    let second = fs.query(&monitor, &token, &spec).unwrap();
    assert_ne!(
        first.folder_id, second.folder_id,
        "linking a new photo to the anchor must evict the stale cache entry"
    );
    assert!(second.member_object_ids.contains(&photo));
}

#[test]
fn a_write_touching_an_unrelated_object_does_not_invalidate() {
    let (_dir, monitor, token, graph, fs) = setup();
    let trip = graph
        .put_node(
            &monitor,
            &token,
            None,
            "trip",
            None,
            json!({"title": "Hawaii"}),
        )
        .unwrap();
    let spec = QuerySpec {
        anchor: Some(trip),
        hop_bound: 1,
        ttl_secs: 300,
        ..Default::default()
    };
    let first = fs.query(&monitor, &token, &spec).unwrap();

    // Unrelated write -- not the anchor, not (yet) a member of this folder.
    graph
        .put_node(
            &monitor,
            &token,
            None,
            "note",
            None,
            json!({"title": "shopping list"}),
        )
        .unwrap();

    let second = fs.query(&monitor, &token, &spec).unwrap();
    assert_eq!(
        first.folder_id, second.folder_id,
        "an unrelated write must not evict a folder it never touched"
    );
}
