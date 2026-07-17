//! docs/31-event-system.md §Testing Strategy: "Backpressure tests attach an
//! artificially slow subscriber to each `BackpressurePolicy` and assert the
//! documented behavior per class (coalescing collapses, buffering
//! drops-with-warning, durable never drops, block stalls the producer)."

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_events::{
    BackpressurePolicy, DeliveryClass, Event, EventBus, EventPayload, SchemaId, SubjectId, Topic,
    TopicKind, TopicPattern,
};

const OWNER: TrustBoundaryId = TrustBoundaryId(1);

fn mint_pair(monitor: &mut CapabilityMonitor) -> (CapabilityToken, CapabilityToken) {
    let root = monitor.mint_root(RightsMask::READ | RightsMask::WRITE, OWNER, None);
    let publisher = monitor
        .cap_derive(&root, RightsMask::WRITE, None, OWNER)
        .unwrap();
    let subscriber = monitor
        .cap_derive(&root, RightsMask::READ, None, OWNER)
        .unwrap();
    (publisher, subscriber)
}

fn topic() -> Topic {
    Topic {
        kind: TopicKind::AgentProgress,
        subject: SubjectId::Agent(7),
        schema_id: SchemaId::new("agent.progress.v1"),
    }
}

fn inline(n: u32) -> EventPayload {
    EventPayload::Inline(serde_json::json!({ "tick": n }))
}

#[test]
fn coalesce_collapses_to_the_latest_value() {
    let mut monitor = CapabilityMonitor::new();
    let (publisher, subscriber) = mint_pair(&mut monitor);
    let topic = topic();

    let bus = EventBus::new(None);
    let sub = bus
        .subscribe(
            &monitor,
            &subscriber,
            OWNER,
            TopicPattern::Exact(topic.clone()),
            DeliveryClass::AtMostOnce,
            BackpressurePolicy::Coalesce,
        )
        .unwrap();

    for n in 1..=5 {
        bus.publish(
            &monitor,
            &publisher,
            OWNER,
            topic.clone(),
            inline(n),
            Vec::new(),
        )
        .unwrap();
    }

    // Only ever "the current value" -- never the four intermediate ticks.
    let received = sub.recv();
    assert_eq!(received.seq, 5);
    assert!(sub.try_recv().is_none());
}

#[test]
fn buffer_drops_oldest_past_capacity_and_counts_it() {
    let mut monitor = CapabilityMonitor::new();
    let (publisher, subscriber) = mint_pair(&mut monitor);
    let topic = topic();

    let bus = EventBus::new(None);
    let sub = bus
        .subscribe(
            &monitor,
            &subscriber,
            OWNER,
            TopicPattern::Exact(topic.clone()),
            DeliveryClass::AtMostOnce,
            BackpressurePolicy::Buffer { capacity: 2 },
        )
        .unwrap();

    for n in 1..=3 {
        bus.publish(
            &monitor,
            &publisher,
            OWNER,
            topic.clone(),
            inline(n),
            Vec::new(),
        )
        .unwrap();
    }

    assert_eq!(bus.dropped_count(sub.id()).unwrap(), 1);
    let first = sub.recv();
    let second = sub.recv();
    assert_eq!(
        (first.seq, second.seq),
        (2, 3),
        "oldest (seq 1) was dropped"
    );
    assert!(sub.try_recv().is_none());
}

#[test]
fn durable_never_drops_and_survives_a_bus_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let (publisher, subscriber) = mint_pair(&mut monitor);
    let topic = topic();

    {
        let bus = EventBus::new(Some(tmp.path().to_path_buf()));
        let sub = bus
            .subscribe(
                &monitor,
                &subscriber,
                OWNER,
                TopicPattern::Exact(topic.clone()),
                DeliveryClass::AtLeastOnce,
                BackpressurePolicy::Durable,
            )
            .unwrap();

        for n in 1..=3 {
            bus.publish(
                &monitor,
                &publisher,
                OWNER,
                topic.clone(),
                inline(n),
                Vec::new(),
            )
            .unwrap();
        }

        // Live delivery still works for a Durable subscription.
        let live: Vec<Event> = (0..3).map(|_| sub.recv()).collect();
        assert_eq!(
            live.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        bus.ack(sub.id(), 2).unwrap();
        // Bus (and its in-memory subscription state) is dropped here --
        // simulating a process restart. The durable log on disk survives.
    }

    let bus2 = EventBus::new(Some(tmp.path().to_path_buf()));
    // The first subscription registered on a fresh bus is always assigned
    // id 1, reproducing the exact file the prior bus instance's first
    // subscription wrote to -- this is the "resume after restart" scenario
    // docs/31 §Recovery Mechanisms describes.
    let sub2 = bus2
        .subscribe(
            &monitor,
            &subscriber,
            OWNER,
            TopicPattern::Exact(topic),
            DeliveryClass::AtLeastOnce,
            BackpressurePolicy::Durable,
        )
        .unwrap();
    assert_eq!(sub2.id().0, 1);

    let replayed = bus2.replay_from(sub2.id(), 0).unwrap();
    assert_eq!(
        replayed.iter().map(|e| e.seq).collect::<Vec<_>>(),
        vec![1, 2, 3],
        "durable log recovered every event even though this fresh subscription never received them live"
    );
}

#[test]
fn ack_is_rejected_for_a_non_at_least_once_subscription() {
    let mut monitor = CapabilityMonitor::new();
    let (publisher, subscriber) = mint_pair(&mut monitor);
    let _ = &publisher;
    let topic = topic();

    let bus = EventBus::new(None);
    let sub = bus
        .subscribe(
            &monitor,
            &subscriber,
            OWNER,
            TopicPattern::Exact(topic),
            DeliveryClass::AtMostOnce,
            BackpressurePolicy::Coalesce,
        )
        .unwrap();

    assert!(bus.ack(sub.id(), 1).is_err());
}

#[test]
fn replay_from_is_rejected_for_a_non_durable_subscription() {
    let mut monitor = CapabilityMonitor::new();
    let (publisher, subscriber) = mint_pair(&mut monitor);
    let _ = &publisher;
    let topic = topic();

    let bus = EventBus::new(None);
    let sub = bus
        .subscribe(
            &monitor,
            &subscriber,
            OWNER,
            TopicPattern::Exact(topic),
            DeliveryClass::AtMostOnce,
            BackpressurePolicy::Buffer { capacity: 4 },
        )
        .unwrap();

    assert!(bus.replay_from(sub.id(), 0).is_err());
}

#[test]
fn block_stalls_the_producer_until_the_subscriber_drains() {
    let mut monitor = CapabilityMonitor::new();
    let (publisher, subscriber) = mint_pair(&mut monitor);
    let topic = topic();

    let bus = Arc::new(EventBus::new(None));
    let sub = bus
        .subscribe(
            &monitor,
            &subscriber,
            OWNER,
            TopicPattern::Exact(topic.clone()),
            DeliveryClass::AtMostOnce,
            BackpressurePolicy::Block,
        )
        .unwrap();

    // Fill the Block subscription's bounded queue (capacity 64) without
    // draining it.
    for n in 1..=64 {
        bus.publish(
            &monitor,
            &publisher,
            OWNER,
            topic.clone(),
            inline(n),
            Vec::new(),
        )
        .unwrap();
    }

    // The 65th publish must stall until we drain one item -- prove it by
    // publishing from a background thread and confirming it hasn't
    // returned yet after a short, generous wait.
    let bus_bg = Arc::clone(&bus);
    let monitor = Arc::new(monitor);
    let monitor_bg = Arc::clone(&monitor);
    let publisher_bg = publisher.clone();
    let topic_bg = topic.clone();
    let handle = thread::spawn(move || {
        bus_bg
            .publish(
                &monitor_bg,
                &publisher_bg,
                OWNER,
                topic_bg,
                inline(65),
                Vec::new(),
            )
            .unwrap();
    });

    thread::sleep(Duration::from_millis(200));
    assert!(
        !handle.is_finished(),
        "publish should still be blocked with a full Block-class queue"
    );

    // Draining one item unblocks the stalled producer.
    let _ = sub.recv();
    handle.join().unwrap();
}
