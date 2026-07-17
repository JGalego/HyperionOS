use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_storage::{ObjectId as StorageObjectId, VersionId, Wal, WalRecord};

use crate::types::{
    now, valid_combination, BackpressurePolicy, DeliveryClass, Event, EventFault, EventPayload,
    SubscriptionId, Topic, TopicPattern,
};

/// Bound on a `Block`-class subscription's queue before `publish` stalls the
/// producer — docs/31 §Algorithms' `block_until_drained`. Not configurable
/// per-subscription today: every `Block` subscriber shares this one small
/// constant, which is enough to prove the "producer stalls rather than lets
/// a tightly-coupled dependent fall behind" property docs/31 asks for.
const BLOCK_CAPACITY: usize = 64;

struct SubscriberState {
    pattern: TopicPattern,
    delivery: DeliveryClass,
    backpressure: BackpressurePolicy,
    holder: CapabilityToken,
    queue: VecDeque<Event>,
    /// docs/31 §Algorithms: "the bus keeps one mailbox slot per `(Topic,
    /// Subscription)`" — keyed by `Topic`, not a single slot per
    /// subscription, so a `Subtree`/`KindScoped` pattern matching several
    /// distinct topics (e.g. one Workspace watching several Panels' own
    /// topics) coalesces each independently instead of one topic's rapid
    /// updates silently clobbering another's still-pending one.
    coalesced: HashMap<Topic, Event>,
    dropped: u64,
    last_acked_seq: u64,
    durable_path: Option<PathBuf>,
    durable_wal: Option<Wal>,
}

struct Mailbox {
    state: Mutex<SubscriberState>,
    not_empty: Condvar,
    not_full: Condvar,
}

struct BusState {
    subscriptions: HashMap<SubscriptionId, Arc<Mailbox>>,
    next_seq: HashMap<Topic, u64>,
    next_sub_id: u64,
    durable_dir: Option<PathBuf>,
}

/// docs/31-event-system.md's Event Bus, translated for this hosted
/// simulator the same way `hyperion-capability`'s own crate doc describes
/// itself: a faithful translation of the design doc's algorithms and
/// security properties, not a byte-for-byte transcription of its
/// pseudocode. Two deliberate translations, each because this workspace has
/// no message-passing kernel to build IPC `Notify` channels on top of:
///
/// - Where docs/31 says "each subscription is a dedicated [30 — IPC
///   Framework] `Notify` channel," this crate gives each subscription a
///   [`Subscription`] handle whose [`Subscription::recv`]/[`Subscription::try_recv`]
///   *are* that channel — blocking/non-blocking receive on a per-subscriber
///   mailbox, rather than a distinct notify-then-fetch protocol. The
///   backpressure/delivery-class properties docs/31 actually cares about
///   (coalescing, bounded buffering, durable spillover, producer stalling)
///   are all real and tested; only the transport substrate differs.
/// - Trie-indexed subtree matching ("a subject's Knowledge-Graph path") is
///   replaced by publisher-supplied `ancestors` on each [`crate::Event`] —
///   this crate does not depend on `hyperion-knowledge-graph` (which will
///   itself depend on this crate to publish `ObjectChanged` events, so the
///   reverse dependency would cycle), so it cannot walk KG hierarchy
///   itself. The publisher, which already knows its own domain's
///   hierarchy, supplies it instead — the same "the core doesn't know about
///   domain kinds" shape `hyperion-capability::CapabilityMonitor::cap_invoke`
///   already establishes by taking a caller-supplied dispatch closure.
/// - "A token that dominates the topic's subject" (docs/31 §Security
///   Considerations) is checked via Trust-Boundary owner equality
///   (`CapabilityToken::origin()` against a caller-supplied `owner:
///   TrustBoundaryId`), not `CapabilityToken::object_id()` — `object_id`
///   names a *kernel* object in `hyperion-capability`'s own namespace,
///   unrelated to a domain crate's own subject ids
///   (`hyperion-knowledge-graph::NodeId`, `AgentId`, ...); the real
///   equality check every domain crate's own read/write path already
///   performs is owner-based (see `authorize_publish`/`authorize_subscribe`'s
///   own doc comments).
///
/// Federated (cross-device) topic propagation (docs/31 §Architecture: "at
/// scale... sharded per device with cross-shard forwarding") is not this
/// crate's concern either: a device that wants a remote peer's events
/// subscribes locally and has `hyperion-federation` forward published
/// events across its own already-real transport, the same layering
/// `hyperion-observability::TelemetryCollector::merge_remote_trace` already
/// uses for cross-device trace merging.
pub struct EventBus {
    state: Mutex<BusState>,
}

impl EventBus {
    /// `durable_dir`: where `Durable`-backpressure subscriptions spill their
    /// append log (docs/31 §Algorithms' "spill to a storage-engine append
    /// log instead of the in-memory queue"). `None` is valid — a bus with
    /// no durable directory configured simply cannot host `Durable`
    /// subscriptions, and `subscribe` reports that explicitly rather than
    /// silently downgrading to a non-durable queue.
    pub fn new(durable_dir: Option<PathBuf>) -> Self {
        EventBus {
            state: Mutex::new(BusState {
                subscriptions: HashMap::new(),
                next_seq: HashMap::new(),
                next_sub_id: 1,
                durable_dir,
            }),
        }
    }

    /// `topic_publish` — docs/31 §Interfaces / APIs. Validates the caller's
    /// capability against `owner` — the subject's Trust-Boundary owner, the
    /// same `owner: u64` every domain record in this workspace already
    /// carries (`hyperion_knowledge_graph::NodeRecord::owner`, `EdgeRecord::owner`,
    /// ...) and the same equality check their own read/write paths already
    /// perform (`token.origin().0 == record.owner`) — then assigns the next
    /// per-topic `seq` and fans out to every matching subscription
    /// independently (§Algorithms: "the *same* `Event` can be delivered
    /// `AtMostOnce`/`Coalesce` to one subscription and `AtLeastOnce`/
    /// `Durable` to another in the same fan-out pass"). This crate does not
    /// use `CapabilityToken::object_id()` for this check: that id names a
    /// *kernel* object in `hyperion-capability`'s own namespace, a
    /// different space entirely from a domain crate's own subject ids
    /// (`hyperion-knowledge-graph::NodeId`, `AgentId`, ...) — see this
    /// module's doc comment.
    pub fn publish(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        owner: TrustBoundaryId,
        topic: Topic,
        payload: EventPayload,
        ancestors: Vec<u64>,
    ) -> Result<u64, EventFault> {
        authorize_publish(monitor, token, owner)?;

        let mut state = self.state.lock().unwrap();
        let seq_slot = state.next_seq.entry(topic.clone()).or_insert(0);
        *seq_slot += 1;
        let seq = *seq_slot;
        let event = Event {
            topic,
            seq,
            timestamp: now(),
            payload,
            ancestors,
        };

        let matching: Vec<Arc<Mailbox>> = state
            .subscriptions
            .values()
            .filter(|mailbox| mailbox.state.lock().unwrap().pattern.matches(&event))
            .cloned()
            .collect();
        drop(state);

        for mailbox in matching {
            deliver(&mailbox, &event);
        }

        Ok(seq)
    }

    /// `subscribe` — docs/31 §Interfaces / APIs, validated per §Pseudocode's
    /// delivery/backpressure compatibility table before anything is
    /// registered. `owner` is ignored for a [`TopicPattern::KindScoped`]
    /// subscription, which is authorized on `GRANT` rights alone (see
    /// `authorize_subscribe`) — pass any value (e.g. the caller's own
    /// origin) in that case.
    pub fn subscribe(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        owner: TrustBoundaryId,
        pattern: TopicPattern,
        delivery: DeliveryClass,
        backpressure: BackpressurePolicy,
    ) -> Result<Subscription, EventFault> {
        if !valid_combination(delivery, backpressure) {
            return Err(EventFault::IncompatibleDeliveryBackpressure(
                delivery,
                backpressure,
            ));
        }
        authorize_subscribe(monitor, token, owner, &pattern)?;

        let mut state = self.state.lock().unwrap();
        let id = SubscriptionId(state.next_sub_id);
        state.next_sub_id += 1;

        let (durable_path, durable_wal) = if matches!(backpressure, BackpressurePolicy::Durable) {
            let dir = state.durable_dir.clone().ok_or_else(|| {
                EventFault::StorageError("no durable_dir configured on this bus".into())
            })?;
            std::fs::create_dir_all(&dir).map_err(|e| EventFault::StorageError(e.to_string()))?;
            let path = dir.join(format!("subscription-{}.wal", id.0));
            let wal =
                Wal::open_for_append(&path).map_err(|e| EventFault::StorageError(e.to_string()))?;
            (Some(path), Some(wal))
        } else {
            (None, None)
        };

        let mailbox = Arc::new(Mailbox {
            state: Mutex::new(SubscriberState {
                pattern,
                delivery,
                backpressure,
                holder: token.clone(),
                queue: VecDeque::new(),
                coalesced: HashMap::new(),
                dropped: 0,
                last_acked_seq: 0,
                durable_path,
                durable_wal,
            }),
            not_empty: Condvar::new(),
            not_full: Condvar::new(),
        });
        state.subscriptions.insert(id, mailbox.clone());
        drop(state);

        Ok(Subscription { id, mailbox })
    }

    pub fn unsubscribe(&self, id: SubscriptionId) -> Result<(), EventFault> {
        let mut state = self.state.lock().unwrap();
        state
            .subscriptions
            .remove(&id)
            .map(|_| ())
            .ok_or(EventFault::NoSuchSubscription)
    }

    /// `ack` — docs/31 §Interfaces / APIs, `AtLeastOnce` only.
    pub fn ack(&self, id: SubscriptionId, seq: u64) -> Result<(), EventFault> {
        let mailbox = self.mailbox_of(id)?;
        let mut st = mailbox.state.lock().unwrap();
        if st.delivery != DeliveryClass::AtLeastOnce {
            return Err(EventFault::AckNotApplicable);
        }
        if seq > st.last_acked_seq {
            st.last_acked_seq = seq;
        }
        Ok(())
    }

    /// `replay_from` — docs/31 §Interfaces / APIs, durable topics only.
    /// Reads the subscription's own append log rather than the live
    /// in-memory queue, so it works even immediately after a bus restart
    /// (§Recovery Mechanisms: "`AtLeastOnce` subscribers resume from
    /// `last_acked_seq` via `replay_from`, reading the durable log").
    pub fn replay_from(&self, id: SubscriptionId, since: u64) -> Result<Vec<Event>, EventFault> {
        let mailbox = self.mailbox_of(id)?;
        let path = {
            let st = mailbox.state.lock().unwrap();
            if !matches!(st.backpressure, BackpressurePolicy::Durable) {
                return Err(EventFault::NotDurable);
            }
            st.durable_path
                .clone()
                .expect("Durable subscription always has a durable_path")
        };
        let records = Wal::replay(&path).map_err(|e| EventFault::StorageError(e.to_string()))?;
        let mut events: Vec<Event> = records
            .into_iter()
            .filter_map(|r| serde_json::from_value::<Event>(r.metadata).ok())
            .filter(|e| e.seq > since)
            .collect();
        events.sort_by_key(|e| e.seq);
        Ok(events)
    }

    /// Count of events dropped so far for `id` under `Buffer` backpressure
    /// (docs/31 §Algorithms: "drop the oldest queued item and log a
    /// warning") — exposed so a caller can surface it to
    /// `hyperion-observability` rather than it only ever reaching stderr.
    pub fn dropped_count(&self, id: SubscriptionId) -> Result<u64, EventFault> {
        let mailbox = self.mailbox_of(id)?;
        let dropped = mailbox.state.lock().unwrap().dropped;
        Ok(dropped)
    }

    fn mailbox_of(&self, id: SubscriptionId) -> Result<Arc<Mailbox>, EventFault> {
        self.state
            .lock()
            .unwrap()
            .subscriptions
            .get(&id)
            .cloned()
            .ok_or(EventFault::NoSuchSubscription)
    }
}

/// A live subscription's receive side — this crate's translation of docs/31
/// §Interfaces / APIs' "delivery itself is not a distinct API: events
/// arrive... as `Notify` frames on the channel opened implicitly by
/// `subscribe`" (see [`EventBus`]'s own doc comment for why).
pub struct Subscription {
    id: SubscriptionId,
    mailbox: Arc<Mailbox>,
}

impl Subscription {
    pub fn id(&self) -> SubscriptionId {
        self.id
    }

    /// Blocks until the next event this subscription's pattern matches is
    /// available. For a `Coalesce` subscription this may skip intermediate
    /// values entirely — by design (docs/31 §Trade-offs: "a `Coalesce`
    /// subscriber only ever sees 'the current value'").
    pub fn recv(&self) -> Event {
        let mut st = self.mailbox.state.lock().unwrap();
        loop {
            if let Some(event) = take_ready(&mut st) {
                self.mailbox.not_full.notify_all();
                return event;
            }
            st = self.mailbox.not_empty.wait(st).unwrap();
        }
    }

    /// Non-blocking `recv` — `None` if nothing is currently pending.
    pub fn try_recv(&self) -> Option<Event> {
        let mut st = self.mailbox.state.lock().unwrap();
        let event = take_ready(&mut st);
        if event.is_some() {
            self.mailbox.not_full.notify_all();
        }
        event
    }
}

fn take_ready(st: &mut SubscriberState) -> Option<Event> {
    match st.backpressure {
        // Which distinct topic's pending slot drains first when several are
        // pending is unspecified (docs/31 §Trade-offs already offers no
        // cross-topic ordering guarantee) -- any one is a valid choice.
        BackpressurePolicy::Coalesce => {
            let key = st.coalesced.keys().next().cloned();
            key.and_then(|k| st.coalesced.remove(&k))
        }
        _ => st.queue.pop_front(),
    }
}

fn deliver(mailbox: &Arc<Mailbox>, event: &Event) {
    let mut st = mailbox.state.lock().unwrap();
    match st.backpressure {
        BackpressurePolicy::Coalesce => {
            // Overwrite this topic's own pending slot instead of queuing --
            // bounds memory to O(distinct matching topics) regardless of
            // publish rate (docs/31 §Algorithms), independently per topic
            // for a Subtree/KindScoped subscription matching several.
            st.coalesced.insert(event.topic.clone(), event.clone());
            mailbox.not_empty.notify_all();
        }
        BackpressurePolicy::Durable => {
            append_durable(&mut st, event);
            st.queue.push_back(event.clone());
            mailbox.not_empty.notify_all();
        }
        BackpressurePolicy::Buffer { capacity } => {
            if st.queue.len() >= capacity as usize {
                st.queue.pop_front();
                st.dropped += 1;
                eprintln!(
                    "hyperion-events: subscription queue at capacity {capacity}, dropped oldest event (topic seq {})",
                    event.seq
                );
            }
            st.queue.push_back(event.clone());
            mailbox.not_empty.notify_all();
        }
        BackpressurePolicy::Block => {
            while st.queue.len() >= BLOCK_CAPACITY {
                st = mailbox.not_full.wait(st).unwrap();
            }
            st.queue.push_back(event.clone());
            mailbox.not_empty.notify_all();
        }
    }
}

fn append_durable(st: &mut SubscriberState, event: &Event) {
    let Some(wal) = st.durable_wal.as_mut() else {
        eprintln!("hyperion-events: durable subscription has no open WAL handle");
        return;
    };
    let metadata = match serde_json::to_value(event) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("hyperion-events: failed to serialize durable event: {e}");
            return;
        }
    };
    let record = WalRecord {
        object_id: StorageObjectId(0),
        prev_version: (event.seq > 1).then(|| VersionId(event.seq - 1)),
        new_version: VersionId(event.seq),
        metadata,
        actor_origin: st.holder.origin().0,
    };
    if let Err(e) = wal.append_and_fsync(&record) {
        eprintln!("hyperion-events: durable append failed: {e}");
    }
}

fn authorize_publish(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    owner: TrustBoundaryId,
) -> Result<(), EventFault> {
    monitor
        .check_rights_ok_result(token, RightsMask::WRITE)
        .map_err(|_| EventFault::Unauthorized)?;
    if token.origin().0 != owner.0 {
        return Err(EventFault::Unauthorized);
    }
    Ok(())
}

/// docs/31 §Security Considerations: "`subscribe` requires a token proving
/// the subscriber already has read authority over that subject." Translated
/// onto this workspace's real (coarse, Trust-Boundary-owner-based) access
/// model rather than `hyperion-capability`'s kernel-object-slot one: a
/// subscriber must present a token from the *same* Trust Boundary as the
/// subject's owner, exactly the equality check every domain crate's own
/// read path already performs (`hyperion_knowledge_graph::KnowledgeGraph::get`,
/// for one). A `TopicPattern::KindScoped` names no single subject, so it is
/// authorized on rights alone: a `GRANT`-bearing token signals the same
/// kind of elevated, cross-subject authority a docs/34 audit sink actually
/// holds, reusing an existing bitflag rather than inventing an "admin"
/// concept.
fn authorize_subscribe(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    owner: TrustBoundaryId,
    pattern: &TopicPattern,
) -> Result<(), EventFault> {
    monitor
        .check_rights_ok_result(token, RightsMask::READ)
        .map_err(|_| EventFault::Unauthorized)?;
    if pattern.is_kind_scoped() {
        if token.rights().contains(RightsMask::GRANT) {
            Ok(())
        } else {
            Err(EventFault::Unauthorized)
        }
    } else if token.origin().0 == owner.0 {
        Ok(())
    } else {
        Err(EventFault::Unauthorized)
    }
}
