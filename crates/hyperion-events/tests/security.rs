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
fn subscribe_from_a_foreign_trust_boundary_is_denied() {
    let mut monitor = CapabilityMonitor::new();
    // Minted for Trust Boundary 1, but the subject's declared owner is Trust Boundary 2.
    let foreign = monitor.mint_root(RightsMask::READ, TrustBoundaryId(1), None);

    let bus = EventBus::new(None);
    let result = bus.subscribe(
        &monitor,
        &foreign,
        TrustBoundaryId(2),
        TopicPattern::Exact(topic()),
        DeliveryClass::AtMostOnce,
        BackpressurePolicy::Coalesce,
    );
    assert_eq!(result.err(), Some(EventFault::Unauthorized));
}

#[test]
fn subscribe_without_read_rights_is_denied_even_for_the_right_owner() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::WRITE, TrustBoundaryId(1), None);
    // Attenuate away READ entirely.
    let write_only = monitor
        .cap_derive(&root, RightsMask::WRITE, None, TrustBoundaryId(1))
        .unwrap();

    let bus = EventBus::new(None);
    let result = bus.subscribe(
        &monitor,
        &write_only,
        TrustBoundaryId(1),
        TopicPattern::Exact(topic()),
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
    let result = bus.publish(
        &monitor,
        &read_only,
        TrustBoundaryId(1),
        topic(),
        EventPayload::Inline(serde_json::json!({"changed": true})),
        Vec::new(),
    );
    assert_eq!(result.err(), Some(EventFault::Unauthorized));
}

#[test]
fn publish_from_a_foreign_trust_boundary_is_denied_even_with_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let writer = monitor.mint_root(RightsMask::WRITE, TrustBoundaryId(1), None);

    let bus = EventBus::new(None);
    // The subject's declared owner is Trust Boundary 2, not 1.
    let result = bus.publish(
        &monitor,
        &writer,
        TrustBoundaryId(2),
        topic(),
        EventPayload::Inline(serde_json::json!({})),
        Vec::new(),
    );
    assert_eq!(result.err(), Some(EventFault::Unauthorized));
}

#[test]
fn kind_scoped_subscription_requires_grant_rights_regardless_of_owner() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::READ, TrustBoundaryId(1), None);

    let bus = EventBus::new(None);
    let denied = bus.subscribe(
        &monitor,
        &root,
        TrustBoundaryId(1),
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
    // Owner is irrelevant for a KindScoped pattern -- pass an unrelated boundary
    // to prove GRANT rights alone are what authorize it.
    let allowed = bus.subscribe(
        &monitor,
        &admin,
        TrustBoundaryId(999),
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

    let bus = EventBus::new(None);
    let at_most_once_durable = bus.subscribe(
        &monitor,
        &token,
        TrustBoundaryId(1),
        TopicPattern::Exact(topic()),
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
        TrustBoundaryId(1),
        TopicPattern::Exact(topic()),
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

    let bus = EventBus::new(None);
    let result = bus.subscribe(
        &monitor,
        &token,
        TrustBoundaryId(1),
        TopicPattern::Exact(topic()),
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
    let result = bus.publish(
        &monitor,
        &token,
        TrustBoundaryId(1),
        topic(),
        EventPayload::Inline(serde_json::json!({})),
        Vec::new(),
    );
    assert_eq!(result.err(), Some(EventFault::Unauthorized));
}
