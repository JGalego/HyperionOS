//! docs/31-event-system.md's own motivating example: "09 — Knowledge Graph
//! object-changed notifications." Proves `KnowledgeGraph::with_events` makes
//! `put_node`/`link`/`unlink`/`delete_node` each publish a real
//! `ObjectChanged` event under the write's own Trust-Boundary owner.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_events::{BackpressurePolicy, DeliveryClass, EventBus, TopicKind, TopicPattern};
use hyperion_knowledge_graph::{EdgeOrigin, KnowledgeGraph};

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    Arc<EventBus>,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(
        RightsMask::READ | RightsMask::WRITE | RightsMask::GRANT,
        TrustBoundaryId(1),
        None,
    );
    let bus = Arc::new(EventBus::new(None));
    (dir, monitor, token, bus)
}

#[test]
fn put_node_publishes_object_changed() {
    let (dir, monitor, token, bus) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl"))
        .unwrap()
        .with_events(bus.clone());

    let sub = bus
        .subscribe(
            &monitor,
            &token,
            token.origin(),
            TopicPattern::KindScoped(TopicKind::ObjectChanged),
            DeliveryClass::AtMostOnce,
            BackpressurePolicy::Buffer { capacity: 16 },
        )
        .unwrap();

    let node = graph
        .put_node(&monitor, &token, None, "Note", None, serde_json::json!({}))
        .unwrap();

    let event = sub.try_recv().expect("put_node should publish an event");
    assert_eq!(event.topic.schema_id.0, "kg.object_changed.v1");
    assert_eq!(event.topic.subject.raw(), node.0);
}

#[test]
fn link_unlink_and_delete_node_each_publish_object_changed() {
    let (dir, monitor, token, bus) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl"))
        .unwrap()
        .with_events(bus.clone());

    let a = graph
        .put_node(&monitor, &token, None, "Note", None, serde_json::json!({}))
        .unwrap();
    let b = graph
        .put_node(&monitor, &token, None, "Note", None, serde_json::json!({}))
        .unwrap();

    let sub = bus
        .subscribe(
            &monitor,
            &token,
            token.origin(),
            TopicPattern::KindScoped(TopicKind::ObjectChanged),
            DeliveryClass::AtMostOnce,
            BackpressurePolicy::Buffer { capacity: 16 },
        )
        .unwrap();

    let outcome = graph
        .link(
            &monitor,
            &token,
            a,
            "related-to",
            b,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "test",
            None,
        )
        .unwrap();
    let edge_id = match outcome {
        hyperion_knowledge_graph::LinkOutcome::Created(id) => id,
        other => panic!("expected Created, got {other:?}"),
    };
    let link_event = sub.try_recv().expect("link should publish an event");
    assert_eq!(link_event.topic.subject.raw(), edge_id.0);

    graph.unlink(&monitor, &token, edge_id).unwrap();
    let unlink_event = sub.try_recv().expect("unlink should publish an event");
    assert_eq!(unlink_event.topic.subject.raw(), edge_id.0);

    graph.delete_node(&monitor, &token, a).unwrap();
    let delete_event = sub.try_recv().expect("delete_node should publish an event");
    assert_eq!(delete_event.topic.subject.raw(), a.0);
}

#[test]
fn a_graph_with_no_wired_bus_still_writes_normally() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(
        RightsMask::READ | RightsMask::WRITE,
        TrustBoundaryId(1),
        None,
    );
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let node = graph
        .put_node(&monitor, &token, None, "Note", None, serde_json::json!({}))
        .unwrap();
    assert!(graph.get(&monitor, &token, node).is_ok());
}
