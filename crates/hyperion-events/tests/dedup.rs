//! docs/31-event-system.md §Testing Strategy: "Idempotency/dedup tests
//! replay `AtLeastOnce` streams with induced duplicates and assert
//! consumer-side correctness" -- docs/31 §Failure Modes: "subscribers are
//! required by contract to dedupe on `(topic, seq)`." This crate does not
//! itself dedupe (that contract lives at the consumer, matching
//! [30 — IPC Framework]'s own call-retry idempotency discipline), so this
//! test proves the building block the contract depends on: `(topic, seq)`
//! uniquely identifies an event even when the same event legitimately
//! arrives twice, once live and once via `replay_from`.

use std::collections::HashSet;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_events::{
    BackpressurePolicy, DeliveryClass, EventBus, EventPayload, SchemaId, SubjectId, Topic,
    TopicKind, TopicPattern,
};

const OWNER: TrustBoundaryId = TrustBoundaryId(1);

#[test]
fn duplicate_delivery_across_live_and_replayed_streams_dedupes_by_topic_and_seq() {
    let tmp = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::READ | RightsMask::WRITE, OWNER, None);
    let topic = Topic {
        kind: TopicKind::ObjectChanged,
        subject: SubjectId::Object(1),
        schema_id: SchemaId::new("kg.node.v1"),
    };

    let bus = EventBus::new(Some(tmp.path().to_path_buf()));
    let sub = bus
        .subscribe(
            &monitor,
            &root,
            OWNER,
            TopicPattern::Exact(topic.clone()),
            DeliveryClass::AtLeastOnce,
            BackpressurePolicy::Durable,
        )
        .unwrap();

    for n in 0..5 {
        bus.publish(
            &monitor,
            &root,
            OWNER,
            topic.clone(),
            EventPayload::Inline(serde_json::json!({ "n": n })),
            Vec::new(),
        )
        .unwrap();
    }

    // Consume the live stream (a real subscriber's normal path)...
    let live: Vec<_> = (0..5).map(|_| sub.recv()).collect();
    // ...and independently pull the exact same events again from the
    // durable log, simulating a redelivery after a reconnect that raced
    // (or a subscriber that never got around to acking before rejoining).
    let replayed = bus.replay_from(sub.id(), 0).unwrap();
    assert_eq!(replayed.len(), 5);

    // A consumer applying the contractual "(topic, seq)" dedupe key must
    // collapse the combined 10 deliveries down to exactly 5 distinct events.
    let mut seen: HashSet<(String, u64)> = HashSet::new();
    let mut applied = 0usize;
    for event in live.iter().chain(replayed.iter()) {
        let key = (event.topic.schema_id.0.clone(), event.seq);
        if seen.insert(key) {
            applied += 1;
        }
    }
    assert_eq!(
        applied, 5,
        "10 raw deliveries must dedupe to 5 distinct (topic, seq) events"
    );
}
