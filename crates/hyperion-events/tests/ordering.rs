//! docs/31-event-system.md §Testing Strategy: "Ordering tests assert
//! per-topic monotonicity under concurrent publishers and explicitly assert
//! the *absence* of a cross-topic ordering guarantee, so a future change
//! cannot silently add a cost nobody asked for."

use std::sync::{Arc, Barrier};
use std::thread;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_events::{
    BackpressurePolicy, DeliveryClass, EventBus, EventPayload, SchemaId, SubjectId, Topic,
    TopicKind, TopicPattern,
};

const OWNER: TrustBoundaryId = TrustBoundaryId(1);

#[test]
fn seq_is_monotonic_per_topic_under_concurrent_publishers() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::READ | RightsMask::WRITE, OWNER, None);
    let topic = Topic {
        kind: TopicKind::ObjectChanged,
        subject: SubjectId::Object(1),
        schema_id: SchemaId::new("kg.node.v1"),
    };

    let bus = Arc::new(EventBus::new(None));
    let sub = bus
        .subscribe(
            &monitor,
            &root,
            OWNER,
            TopicPattern::Exact(topic.clone()),
            DeliveryClass::AtMostOnce,
            BackpressurePolicy::Buffer { capacity: 64 },
        )
        .unwrap();

    let monitor = Arc::new(monitor);
    let publisher = Arc::new(root.clone());
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let bus = Arc::clone(&bus);
            let monitor = Arc::clone(&monitor);
            let publisher = Arc::clone(&publisher);
            let topic = topic.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                for n in 0..8 {
                    bus.publish(
                        &monitor,
                        &publisher,
                        OWNER,
                        topic.clone(),
                        EventPayload::Inline(serde_json::json!({ "n": n })),
                        Vec::new(),
                    )
                    .unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }

    let mut seqs = Vec::new();
    while let Some(event) = sub.try_recv() {
        seqs.push(event.seq);
    }
    seqs.sort_unstable();
    let expected: Vec<u64> = (1..=64).collect();
    assert_eq!(
        seqs, expected,
        "64 publishes across 8 threads must produce exactly seq 1..=64, no gaps, no duplicates"
    );
}

#[test]
fn no_cross_topic_ordering_guarantee_is_offered() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::READ | RightsMask::WRITE, OWNER, None);

    let topic_a = Topic {
        kind: TopicKind::ObjectChanged,
        subject: SubjectId::Object(1),
        schema_id: SchemaId::new("a"),
    };
    let topic_b = Topic {
        kind: TopicKind::ObjectChanged,
        subject: SubjectId::Object(1),
        schema_id: SchemaId::new("b"),
    };

    let bus = EventBus::new(None);
    // Publish several events on topic B first, then one on topic A.
    for _ in 0..5 {
        bus.publish(
            &monitor,
            &root,
            OWNER,
            topic_b.clone(),
            EventPayload::Inline(serde_json::json!({})),
            Vec::new(),
        )
        .unwrap();
    }
    let seq_a = bus
        .publish(
            &monitor,
            &root,
            OWNER,
            topic_a,
            EventPayload::Inline(serde_json::json!({})),
            Vec::new(),
        )
        .unwrap();

    // Topic A's own sequence starts fresh at 1 regardless of how many
    // events topic B has already accumulated -- there is no shared global
    // counter, i.e. no cross-topic ordering claim.
    assert_eq!(seq_a, 1);
}
