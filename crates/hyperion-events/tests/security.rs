//! docs/31-event-system.md §Testing Strategy: "Security tests attempt to
//! subscribe to topics without a dominating capability and assert the
//! discovery-denial behavior."

use std::time::Duration;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_events::{
    BackpressurePolicy, DeliveryClass, EventBus, EventFault, EventPayload, SchemaId, SubjectId,
    Topic, TopicKind, TopicPattern,
};

fn topic() -> Topic {
    Topic {
        kind: TopicKind::ObjectChanged,
        subject: SubjectId::Object(42),
        schema_id: SchemaId::new("kg.node.v1"),
    }
}

#[test]
fn subscribe_without_dominating_capability_is_denied() {
    let mut monitor = CapabilityMonitor::new();
    // A token for a *different* object (7, not 42) -- does not dominate the topic's subject.
    let foreign = monitor.mint_root(RightsMask::READ, TrustBoundaryId(1), None);
    assert_ne!(foreign.object_id().0, 42);

    let bus = EventBus::new(None);
    let result = bus.subscribe(
        &monitor,
        &foreign,
        TopicPattern::Exact(topic()),
        DeliveryClass::AtMostOnce,
        BackpressurePolicy::Coalesce,
    );
    assert_eq!(result.err(), Some(EventFault::Unauthorized));
}

#[test]
fn subscribe_without_read_rights_is_denied_even_for_the_right_object() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::WRITE, TrustBoundaryId(1), None);
    // Attenuate away READ entirely.
    let write_only = monitor
        .cap_derive(&root, RightsMask::WRITE, None, TrustBoundaryId(1))
        .unwrap();

    let bus = EventBus::new(None);
    let topic = Topic {
        kind: TopicKind::ObjectChanged,
        subject: SubjectId::Object(write_only.object_id().0),
        schema_id: SchemaId::new("kg.node.v1"),
    };
    let result = bus.subscribe(
        &monitor,
        &write_only,
        TopicPattern::Exact(topic),
        DeliveryClass::AtMostOnce,
        BackpressurePolicy::Coalesce,
    );
    assert_eq!(result.err(), Some(EventFault::Unauthorized));
}

#[test]
fn publish_without_write_rights_is_denied() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(
        RightsMask::READ | RightsMask::WRITE,
        TrustBoundaryId(1),
        None,
    );
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(1))
        .unwrap();

    let bus = EventBus::new(None);
    let topic = Topic {
        kind: TopicKind::ObjectChanged,
        subject: SubjectId::Object(read_only.object_id().0),
        schema_id: SchemaId::new("kg.node.v1"),
    };
    let result = bus.publish(
        &monitor,
        &read_only,
        topic,
        EventPayload::Inline(serde_json::json!({"changed": true})),
        Vec::new(),
    );
    assert_eq!(result.err(), Some(EventFault::Unauthorized));
}

#[test]
fn kind_scoped_subscription_requires_grant_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::READ, TrustBoundaryId(1), None);

    let bus = EventBus::new(None);
    let denied = bus.subscribe(
        &monitor,
        &root,
        TopicPattern::KindScoped(TopicKind::AgentProgress),
        DeliveryClass::AtMostOnce,
        BackpressurePolicy::Buffer { capacity: 8 },
    );
    assert_eq!(denied.err(), Some(EventFault::Unauthorized));

    let admin = monitor.mint_root(
        RightsMask::READ | RightsMask::GRANT,
        TrustBoundaryId(1),
        None,
    );
    let allowed = bus.subscribe(
        &monitor,
        &admin,
        TopicPattern::KindScoped(TopicKind::AgentProgress),
        DeliveryClass::AtMostOnce,
        BackpressurePolicy::Buffer { capacity: 8 },
    );
    assert!(allowed.is_ok());
}

#[test]
fn incompatible_delivery_backpressure_combinations_are_rejected() {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::READ, TrustBoundaryId(1), None);
    let topic = Topic {
        kind: TopicKind::ObjectChanged,
        subject: SubjectId::Object(token.object_id().0),
        schema_id: SchemaId::new("kg.node.v1"),
    };

    let bus = EventBus::new(None);
    let at_most_once_durable = bus.subscribe(
        &monitor,
        &token,
        TopicPattern::Exact(topic.clone()),
        DeliveryClass::AtMostOnce,
        BackpressurePolicy::Durable,
    );
    assert!(matches!(
        at_most_once_durable,
        Err(EventFault::IncompatibleDeliveryBackpressure(
            DeliveryClass::AtMostOnce,
            BackpressurePolicy::Durable
        ))
    ));

    let at_least_once_coalesce = bus.subscribe(
        &monitor,
        &token,
        TopicPattern::Exact(topic),
        DeliveryClass::AtLeastOnce,
        BackpressurePolicy::Coalesce,
    );
    assert!(matches!(
        at_least_once_coalesce,
        Err(EventFault::IncompatibleDeliveryBackpressure(
            DeliveryClass::AtLeastOnce,
            BackpressurePolicy::Coalesce
        ))
    ));
}

#[test]
fn durable_subscription_without_a_configured_durable_dir_is_rejected() {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::READ, TrustBoundaryId(1), None);
    let topic = Topic {
        kind: TopicKind::ObjectChanged,
        subject: SubjectId::Object(token.object_id().0),
        schema_id: SchemaId::new("kg.node.v1"),
    };

    let bus = EventBus::new(None);
    let result = bus.subscribe(
        &monitor,
        &token,
        TopicPattern::Exact(topic),
        DeliveryClass::AtLeastOnce,
        BackpressurePolicy::Durable,
    );
    assert!(matches!(result, Err(EventFault::StorageError(_))));
}

#[test]
fn revoked_publisher_can_no_longer_publish() {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(
        RightsMask::WRITE,
        TrustBoundaryId(1),
        Some(Duration::from_secs(60)),
    );
    monitor.cap_revoke(&token);

    let bus = EventBus::new(None);
    let topic = Topic {
        kind: TopicKind::ObjectChanged,
        subject: SubjectId::Object(token.object_id().0),
        schema_id: SchemaId::new("kg.node.v1"),
    };
    let result = bus.publish(
        &monitor,
        &token,
        topic,
        EventPayload::Inline(serde_json::json!({})),
        Vec::new(),
    );
    assert_eq!(result.err(), Some(EventFault::Unauthorized));
}
