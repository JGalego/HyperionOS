use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs()
}

/// docs/31-event-system.md's four topic families. `Custom` covers anything
/// a plugin-contributed Semantic Object type needs that doesn't fit the
/// four built-ins — the same open-extensibility shape
/// `hyperion-knowledge-graph::ObjectType` already uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TopicKind {
    ObjectChanged,
    AgentProgress,
    DeviceLifecycle,
    WorkspaceTrigger,
    Custom,
}

/// Payload schema identity, shared convention with docs/30-ipc-framework.md.
/// Open (`String`), not a closed enum, for the same reason
/// `hyperion-knowledge-graph::ObjectType` is open: new Semantic Object /
/// Capability payload shapes must not require a workspace-wide schema
/// migration to introduce.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SchemaId(pub String);

impl SchemaId {
    pub fn new(id: impl Into<String>) -> Self {
        SchemaId(id.into())
    }
}

/// docs/31 §Data Structures' `SubjectId`. This crate deliberately does not
/// depend on `hyperion-knowledge-graph`/`hyperion-scheduler`/`hyperion-intent`/
/// `hyperion-device` to define these variants in terms of their own id
/// types — the same "the core doesn't know about domain kinds" precedent
/// `hyperion-capability::CapabilityMonitor::cap_invoke` already establishes
/// (it dispatches by a caller-supplied closure rather than hardcoding
/// `Object::Device`/`Object::Thread`). Each domain crate is responsible for
/// wrapping its own id (`NodeId`, `AgentId`, `IntentId`, `DeviceId` are all,
/// today, thin `u64` newtypes) into the matching variant here, and for
/// minting a capability token whose `object_id().0` equals that same raw
/// `u64` — which is exactly what `authorize_publish`/`authorize_subscribe`
/// (see `bus.rs`) check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SubjectId {
    Object(u64),
    ObjectSubtree(u64),
    Agent(u64),
    Intent(u64),
    Device(u64),
}

impl SubjectId {
    /// The raw id this subject wraps, regardless of variant — what a
    /// presented capability token's `object_id()` must equal to dominate
    /// this subject.
    pub fn raw(&self) -> u64 {
        match self {
            SubjectId::Object(id)
            | SubjectId::ObjectSubtree(id)
            | SubjectId::Agent(id)
            | SubjectId::Intent(id)
            | SubjectId::Device(id) => *id,
        }
    }
}

/// docs/31 §Data Structures' `Topic` — a structured key, never a flat
/// string namespace.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Topic {
    pub kind: TopicKind,
    pub subject: SubjectId,
    pub schema_id: SchemaId,
}

impl Topic {
    pub fn new(kind: TopicKind, subject: SubjectId, schema_id: impl Into<String>) -> Self {
        Topic {
            kind,
            subject,
            schema_id: SchemaId::new(schema_id),
        }
    }
}

/// docs/31 §Data Structures' `EventPayload` — inline for small events, or a
/// reference plus version for large Semantic Object payloads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EventPayload {
    Inline(serde_json::Value),
    ObjectRef { object: u64, version: u64 },
}

/// One event on the bus. `seq` is monotonic per-`Topic` only (see
/// docs/31 §Trade-offs — no global cross-topic order is offered).
/// `ancestors` names every `SubjectId::raw()` this event's subject is
/// nested under (e.g. a Knowledge-Graph containment chain) so that a
/// `TopicPattern::Subtree` subscription can match without this crate
/// itself needing Knowledge-Graph traversal knowledge — the publisher,
/// which already knows its own domain's hierarchy, supplies it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub topic: Topic,
    pub seq: u64,
    pub timestamp: u64,
    pub payload: EventPayload,
    pub ancestors: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryClass {
    AtMostOnce,
    AtLeastOnce,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackpressurePolicy {
    Coalesce,
    Buffer { capacity: u32 },
    Durable,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SubscriptionId(pub u64);

/// docs/31 §Data Structures' `TopicPattern` (named inline there as the
/// `Subscription.pattern` field's type).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TopicPattern {
    /// Matches exactly one `Topic`.
    Exact(Topic),
    /// Matches `kind` where the event's subject *is* `root` or `root` is
    /// among the event's declared `ancestors` — docs/31 §Algorithms'
    /// "notify me of any change under this project."
    Subtree { kind: TopicKind, root: SubjectId },
    /// Matches every event of `kind`, regardless of subject. Reserved for
    /// holders of a `GRANT`-bearing token (see `bus::authorize_subscribe`)
    /// since no single dominated subject can justify kind-wide visibility —
    /// the canonical caller is docs/34's audit sink.
    KindScoped(TopicKind),
}

impl TopicPattern {
    pub(crate) fn matches(&self, event: &Event) -> bool {
        match self {
            TopicPattern::Exact(topic) => &event.topic == topic,
            TopicPattern::Subtree { kind, root } => {
                event.topic.kind == *kind
                    && (event.topic.subject.raw() == root.raw()
                        || event.ancestors.contains(&root.raw()))
            }
            TopicPattern::KindScoped(kind) => event.topic.kind == *kind,
        }
    }

    /// Whether this pattern is authorized on rights alone (`KindScoped`, see
    /// `bus::authorize_subscribe`) or must additionally match the declared
    /// Trust-Boundary owner of the subject it names.
    pub(crate) fn is_kind_scoped(&self) -> bool {
        matches!(self, TopicPattern::KindScoped(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EventFault {
    #[error("capability does not dominate this topic's subject")]
    Unauthorized,
    #[error("delivery class {0:?} is incompatible with backpressure policy {1:?}")]
    IncompatibleDeliveryBackpressure(DeliveryClass, BackpressurePolicy),
    #[error("no such subscription")]
    NoSuchSubscription,
    #[error("ack is only meaningful for an AtLeastOnce subscription")]
    AckNotApplicable,
    #[error("replay_from requires a Durable subscription")]
    NotDurable,
    #[error("durable log I/O failure: {0}")]
    StorageError(String),
}

/// docs/31 §Pseudocode's `subscribe` validity table: `Coalesce` only ever
/// makes sense for `AtMostOnce`, `Durable` only ever makes sense for
/// `AtLeastOnce`; `Buffer`/`Block` are valid under either.
pub(crate) fn valid_combination(delivery: DeliveryClass, backpressure: BackpressurePolicy) -> bool {
    matches!(
        (delivery, backpressure),
        (DeliveryClass::AtMostOnce, BackpressurePolicy::Coalesce)
            | (DeliveryClass::AtLeastOnce, BackpressurePolicy::Durable)
            | (_, BackpressurePolicy::Buffer { .. })
            | (_, BackpressurePolicy::Block)
    )
}
