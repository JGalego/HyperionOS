//! Hyperion system-wide publish/subscribe Event Bus — docs/31-event-system.md,
//! the L2 Platform Service every other crate's own doc comment has, until
//! now, only ever cited as "not built": [`hyperion-netstack`]'s
//! `web.entity.resolved`, [`hyperion-workspace`]'s live incremental
//! re-render, [`hyperion-coordination`]'s progress/escalation broadcast,
//! [`hyperion-explainability`]'s `best_effort_reconstruction`, and
//! [`hyperion-semantic-fs`]'s live `VirtualFolder` invalidation all name
//! this exact document as their missing dependency.
//!
//! Real: every one of docs/31's core properties, translated for a
//! single-process hosted simulator (see [`bus::EventBus`]'s own doc comment
//! for the two deliberate translations — a `Subscription` handle standing
//! in for a dedicated IPC `Notify` channel, and publisher-supplied
//! `ancestors` standing in for KG-path trie indexing, since this crate
//! cannot depend on `hyperion-knowledge-graph` without cycling back through
//! it):
//!
//! - **Capability-scoped pub/sub** (docs/31 §Security Considerations):
//!   `publish` requires a token that dominates the topic's subject with
//!   `WRITE`; `subscribe` requires one that dominates it with `READ` (or,
//!   for a kind-wide [`types::TopicPattern::KindScoped`] subscription like a
//!   docs/34 audit sink, a token carrying `GRANT`). Topic existence is
//!   never a discovery channel: an unauthorized `subscribe` is rejected the
//!   same way an unauthorized capability check anywhere else in this
//!   workspace is.
//! - **Per-subscription delivery class × backpressure policy**
//!   (docs/31 §Data Structures / §Algorithms), each independently real:
//!   `Coalesce` (one mailbox slot, latest-wins), `Buffer { capacity }`
//!   (bounded queue, drop-oldest with a logged warning), `Durable` (spills
//!   to a real `hyperion-storage`-backed append log so a slow audit
//!   consumer never loses an event), `Block` (the producer genuinely
//!   stalls until the subscriber drains). `subscribe` rejects the two
//!   combinations docs/31's own pseudocode says can never make sense
//!   (`AtMostOnce`+`Durable`, `AtLeastOnce`+`Coalesce`).
//! - **Per-topic monotonic `seq`, no cross-topic ordering claim**
//!   (docs/31 §Trade-offs) — enforced by keying the sequence counter on the
//!   full `Topic`, not globally.
//! - **`AtLeastOnce` recovery via `replay_from`** (docs/31 §Recovery
//!   Mechanisms): reads the subscription's own durable log directly, so it
//!   works even immediately after this process restarts and the
//!   in-memory bus state is gone — only `Durable`-backpressure
//!   subscriptions can do this, matching "recovery strategy matches what
//!   was actually promised."
//!
//! Deliberately out of this crate's scope, and why: real cross-device topic
//! federation (docs/31 §Architecture's per-device sharding) is a
//! `hyperion-federation` transport concern layered on top of a local bus,
//! not a distributed protocol this crate implements itself — see the
//! [`bus::EventBus`] doc comment. This crate has no opinion on *what* a
//! `payload`'s JSON shape means for a given `schema_id`; that convention is
//! owned by each publisher/subscriber pair, the same way `hyperion-storage`
//! has no opinion on what a `metadata` value means.

mod bus;
mod types;

pub use bus::{EventBus, Subscription};
pub use types::{
    BackpressurePolicy, DeliveryClass, Event, EventFault, EventPayload, SchemaId, SubjectId,
    SubscriptionId, Topic, TopicKind, TopicPattern,
};
