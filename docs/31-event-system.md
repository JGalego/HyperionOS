# Event System

## Purpose

Specifies Hyperion's system-wide publish/subscribe event backbone — the L2 Platform Service
[02 — Core Architecture §1](02-core-architecture.md#1-layered-system-view) lists alongside the
Capability Registry, Plugin Framework, and Storage Engine. Where [30 — IPC
Framework](30-ipc-framework.md) is point-to-point (one caller, one callee, request correlated to
reply), the Event System is broadcast: many-to-many, decoupled, subscriber set unknown to the
publisher. Every subsystem that needs to react to something happening elsewhere in the system
without polling for it — [09 — Knowledge Graph](09-knowledge-graph.md) object-changed
notifications, [11 — Agent Runtime](11-agent-runtime.md) / [12 — Multi-Agent
Coordination](12-multi-agent-coordination.md) progress events, [20 — Device
Framework](20-device-framework.md) connect/disconnect, and [13 — Dynamic UI
Runtime](13-dynamic-ui-runtime.md)'s live workspace updates — is built on this document, not on a
bespoke callback mechanism of its own.

## Motivation

Hyperion's UI is generated and torn down per Intent
([01 §8](01-vision-and-philosophy.md#8-visual-interfaces-still-matter)), which means a Workspace
routinely needs to reflect state it did not itself produce — a Research Agent's result landing, a
Semantic Object another Agent just edited, a phone that just connected. Wiring each of these as a
direct IPC call from producer to every possible consumer is unworkable: the producer (an Agent, a
driver, the Knowledge Graph) cannot know at write time which Workspaces, audit sinks, or
coordination processes care about this particular change. The Event System exists to decouple
*that something happened* from *who needs to know*, while still honoring
[02 §4's](02-core-architecture.md#4-design-invariants) "no silent authority" and "everything is
auditable" invariants — a broadcast mechanism is exactly where those invariants are easiest to
accidentally violate (any topic could otherwise leak into an uninvolved Workspace), so
capability-scoping the pub/sub layer itself, not just IPC, is a first-class requirement rather than
an afterthought.

### Relationship to 30 — IPC Framework and 07 — Context Propagation

These three mechanisms are easy to conflate, so the boundary is stated explicitly here:

| | Cardinality | Direction | Carries | Typical use |
|---|---|---|---|---|
| [30 — IPC](30-ipc-framework.md) | 1:1 | caller knows callee | a Capability invocation's request/response, or a targeted one-way notify | "invoke this Capability," "tell my one caller I'm 40% done" |
| [07 — Context Propagation](07-context-propagation.md) | 1:1, attached | carried *with* an invocation | the task-scoped Context Bundle (active objects, recent Intents, Memory) an Intent or Agent invocation needs to resolve ambiguity | "continue yesterday's work" needs the prior Context Bundle, not a broadcast |
| 31 — Events (this document) | 1:N, decoupled | publisher does not know subscribers | a fact — an object changed, an Agent progressed, a device appeared | a Workspace reacting to results it did not request |

Concretely: when a Coaching Agent asks a Capability a question, the question and its Context Bundle
travel over [30](30-ipc-framework.md) under [07](07-context-propagation.md)'s propagation rules;
when that Capability later finishes and several unrelated Workspaces, an audit sink, and a
coordination process all need to hear about it, that fan-out is this document's job, not a
broadcast bolted onto IPC or Context Propagation.

## Architecture

```
   Publisher (KG write, Agent runtime, Device Framework, ...)
            │  topic_publish(publish_cap, topic, event)
            ▼
   ┌────────────────────────────────────────────────────────┐
   │                      Event Bus  (L2)                      │
   │  ┌────────────┐  ┌───────────────┐  ┌──────────────────┐  │
   │  │ Topic Index │→│ Subscription   │→│ Backpressure /      │  │
   │  │ (kind,       │  │ Matcher        │  │ Delivery-Class      │  │
   │  │ subject, sub-│  │ (capability-   │  │ Manager (per-sub    │  │
   │  │ tree prefix) │  │  scoped)       │  │  policy, §Data)      │  │
   │  └────────────┘  └───────────────┘  └─────────┬────────────┘  │
   └───────────────────────────────────────────────┼───────────────┘
                                                     │ fan-out, one
                                                     │ decision per
                                                     ▼ subscription
     ┌───────────────┐   ┌────────────────┐   ┌──────────────────┐
     │ 13 — Dynamic UI │   │ 34 — Observ-   │   │ 12 — Multi-Agent  │
     │ Runtime         │   │ ability sink   │   │ Coordination       │
     │ (AtMostOnce,    │   │ (AtLeastOnce,  │   │ (AtLeastOnce or    │
     │ Coalesce)       │   │ Durable)       │   │ AtMostOnce, by     │
     │                 │   │                │   │ event kind)         │
     └───────────────┘   └────────────────┘   └──────────────────┘
              delivered via 30 — IPC Framework's ipc_notify, per subscriber
```

Each subscription is, underneath, a dedicated [30 — IPC Framework](30-ipc-framework.md) `Notify`
channel from the bus to that one subscriber: the Event System is the fan-out and policy layer built
*on top of* point-to-point IPC, not a replacement transport. This is why the bus can apply a
different delivery class per subscription for the *same* event (§Algorithms) — the guarantee is a
property of the edge from bus to subscriber, not of the event itself. The bus is itself a
supervised L2 service with the same crash/restart semantics as any other
([03 — Kernel Architecture](03-kernel-architecture.md#recovery-mechanisms)); at scale, or across a
federation, it is sharded per device with cross-shard forwarding for subscriptions that span
devices, consistent with [21 — Distributed Execution](21-distributed-execution.md).

## Data Structures

```rust
/// Structured topic key — never a flat string namespace. Prevents both
/// topic-string collisions and the wildcard-hack sprawl a flat namespace
/// eventually accumulates.
struct Topic {
    kind: TopicKind,        // ObjectChanged | AgentProgress | DeviceLifecycle | WorkspaceTrigger | Custom(SchemaId)
    subject: SubjectId,     // ObjectId | AgentId | IntentId | DeviceId | SubjectSubtree
    schema_id: SchemaId,    // payload schema, shared convention with 30-ipc-framework.md
}

enum SubjectId {
    Object(ObjectId),           // 09-knowledge-graph.md
    ObjectSubtree(ObjectId),    // matches an object and its KG descendants
    Agent(AgentId),             // 11-agent-runtime.md
    Intent(IntentId),           // 05-intent-engine.md
    Device(DeviceId),           // 20-device-framework.md
}

/// One event on the bus. `seq` is monotonic per-Topic only — the bus makes
/// no global ordering claim across topics (see Trade-offs).
struct Event {
    topic: Topic,
    seq: u64,
    timestamp: Instant,
    payload: EventPayload,   // inline for small events, or an ObjectRef + version
                              // for large Semantic Object payloads (zero-copy via 30)
}

enum DeliveryClass {
    AtMostOnce,    // may drop or coalesce stale events; never redelivered after a bus restart
    AtLeastOnce,   // durable until acked; redelivered from the durable log on reconnect
}

enum BackpressurePolicy {
    Coalesce,                 // keep only the latest event per Topic key (UI-class)
    Buffer { capacity: u32 }, // bounded queue, drop-oldest with a logged warning past capacity
    Durable,                  // spill to 28-storage-engine.md append log; never silently drops
    Block,                    // rare: producer stalls until the subscriber catches up
}

struct Subscription {
    id: SubscriptionId,
    holder: CapabilityToken,   // must dominate the topic's subject (§Security)
    pattern: TopicPattern,     // exact Topic or a subtree/kind-scoped pattern
    delivery: DeliveryClass,
    backpressure: BackpressurePolicy,
    last_acked_seq: u64,       // meaningful only for AtLeastOnce
}
```

## Algorithms

**Publish and per-subscription fan-out.** `topic_publish` validates the caller's publish capability
against the topic's subject, assigns the next per-topic `seq`, and hands the event to every
matching `Subscription` independently. This is the key design decision: the *same* `Event` can be
delivered `AtMostOnce`/`Coalesce` to a Workspace subscription and `AtLeastOnce`/`Durable` to an
audit subscription in the same fan-out pass, because the guarantee lives on the subscription, not
the event.

**Subscription matching.** Topics are indexed by `(kind, subject)` in a trie keyed on the
subject's Knowledge-Graph path where applicable, so a subtree subscription ("notify me of any
change under this project") is a single prefix lookup, not a scan of all subscriptions — this is
what keeps fan-out cost proportional to matching subscriptions, not total subscriptions, as the
Knowledge Graph grows.

**Coalescing (AtMostOnce/Coalesce).** The bus keeps one mailbox slot per `(Topic, Subscription)`; a
new event overwrites the pending slot instead of queuing, and a delivery is enqueued via [30 —
IPC](30-ipc-framework.md)'s `ipc_notify` only if one is not already pending. This bounds memory to
`O(subscriptions)` regardless of event frequency and is what lets a rapidly updating progress topic
([11 — Agent Runtime](11-agent-runtime.md)) never build an unbounded backlog against a Workspace
that only ever wants "current state."

**Backpressure escalation.** The bus tracks queue depth per subscription. Below a low watermark,
delivery is immediate; above a high watermark, the subscription's own `BackpressurePolicy` applies
— `Coalesce` subscriptions simply keep collapsing (backpressure is not even observable to them by
construction); `Buffer` subscriptions drop the oldest queued item and log a warning; `Durable`
subscriptions (the [34 — Observability & Telemetry](34-observability-telemetry.md) audit sink is
the canonical example) spill to a [28 — Storage Engine](28-storage-engine.md)-backed append log
instead of the in-memory queue, so a temporarily slow audit consumer never causes an event to be
dropped; `Block` is reserved for tightly coupled internal producers that would rather stall than
let a dependent fall arbitrarily far behind.

## Interfaces / APIs

```
topic_publish(cap: CapabilityToken, topic: Topic, payload: EventPayload) -> Result<u64, EventFault>
subscribe(cap: CapabilityToken, pattern: TopicPattern, delivery: DeliveryClass,
          backpressure: BackpressurePolicy) -> Result<SubscriptionId, EventFault>
unsubscribe(id: SubscriptionId) -> Result<(), EventFault>
ack(id: SubscriptionId, seq: u64) -> Result<(), EventFault>                     // AtLeastOnce only
replay_from(id: SubscriptionId, since: u64) -> Result<EventStream, EventFault>  // durable topics only
```

Delivery itself is not a distinct API: events arrive at a subscriber as [30 — IPC
Framework](30-ipc-framework.md) `Notify` frames on the channel opened implicitly by `subscribe`.

## Pseudocode

```rust
// Rejects exactly the two combinations `publish` cannot service: Coalesce only ever makes
// sense for AtMostOnce (an AtLeastOnce subscriber that coalesces would silently lose the
// "never drops" guarantee it asked for), and Durable only ever makes sense for AtLeastOnce
// (an AtMostOnce subscriber has no use for a durable replay log it will never be redelivered
// from). Buffer and Block are valid under either delivery class. This is what makes
// `publish`'s later `unreachable!()` actually true rather than an unbacked assertion.
fn subscribe(cap: CapabilityToken, pattern: TopicPattern, delivery: DeliveryClass,
             backpressure: BackpressurePolicy) -> Result<SubscriptionId, EventFault>
{
    let valid = match (delivery, &backpressure) {
        (DeliveryClass::AtMostOnce, BackpressurePolicy::Coalesce) => true,
        (DeliveryClass::AtLeastOnce, BackpressurePolicy::Durable) => true,
        (_, BackpressurePolicy::Buffer { .. }) => true,
        (_, BackpressurePolicy::Block) => true,
        _ => false,   // (AtMostOnce, Durable) and (AtLeastOnce, Coalesce) — rejected here
    };
    if !valid {
        return Err(EventFault::IncompatibleDeliveryBackpressure);
    }
    authorize_subscribe(&cap, &pattern)?;
    Ok(bus_register(cap, pattern, delivery, backpressure))
}

fn publish(bus: &mut EventBus, cap: CapabilityToken, topic: Topic, payload: EventPayload)
    -> Result<u64, EventFault>
{
    authorize_publish(&cap, &topic)?;                      // capability must dominate subject
    let seq = bus.next_seq(&topic);
    let event = Event { topic: topic.clone(), seq, timestamp: now(), payload };

    for sub in bus.index.matching(&topic) {                // trie lookup, §Algorithms
        match (sub.delivery, &sub.backpressure) {
            (DeliveryClass::AtMostOnce, BackpressurePolicy::Coalesce) => {
                bus.mailbox(sub.id).replace(event.clone());   // overwrite, never queue
                if !bus.mailbox(sub.id).delivery_pending() {
                    ipc_notify(&sub.channel, event.clone().into())?;   // 30-ipc-framework.md
                }
            }
            (DeliveryClass::AtLeastOnce, BackpressurePolicy::Durable) => {
                storage_append(sub.durable_log(), &event)?;    // 28-storage-engine.md
                try_deliver_or_defer(&sub, &event);            // redelivered until acked
            }
            (_, BackpressurePolicy::Buffer { capacity }) => {
                if bus.queue(sub.id).len() >= *capacity as usize {
                    bus.queue(sub.id).drop_oldest();
                    log_warn(EventFault::Dropped { sub: sub.id, seq });
                }
                bus.queue(sub.id).push(event.clone());
                drain_queue(&sub);
            }
            (_, BackpressurePolicy::Block) => {
                block_until_drained(&sub)?;                    // producer stalls, by design
                ipc_notify(&sub.channel, event.clone().into())?;
            }
            _ => unreachable!("delivery/backpressure combination validated at subscribe()"),
        }
    }
    Ok(seq)
}
```

## Security Considerations

Publishing and subscribing are both capability-gated exactly like an IPC channel open
([30](30-ipc-framework.md#architecture)): `authorize_publish` requires a token that dominates the
topic's subject, and `subscribe` requires a token proving the subscriber already has read authority
over that subject — a process cannot learn a Semantic Object changed, or that a device connected,
by subscribing to its topic if it could not already read that object or device directly. Topic
existence itself is therefore not a discovery channel: subscribing to an unauthorized topic returns
the same denial [30 — IPC Framework](30-ipc-framework.md#architecture) uses for a nonexistent
endpoint. Payloads are further redacted per-subscriber where a topic's subject is broader than any
single subscriber's grant (a device-lifecycle topic scoped to "all peripherals" must not leak a
specific device's identifying details to a subscriber who only holds a capability over the topic,
not the device itself) — enforced against [16 — Privacy Architecture](16-privacy-architecture.md)
consent scoping at delivery time, not only at subscribe time. `AtLeastOnce`/`Durable` topics feeding
[34 — Observability & Telemetry](34-observability-telemetry.md) and [17 — Threat
Model](17-threat-model.md) are hash-chained in their append log so a compromised process cannot
retroactively edit the audit trail without detection. Publish-rate quotas (from [04 —
Scheduler](04-scheduler.md) resource ledgers) bound a compromised or buggy producer's ability to
flood the bus as a denial-of-service vector against every subscriber at once.

## Failure Modes

- **Bus process crash.** The bus is a supervised service restarted from persisted `Subscription`
  records (themselves Semantic-Object-like, not transient state), per
  [03's](03-kernel-architecture.md#recovery-mechanisms) microreboot pattern. `AtMostOnce` events in
  flight at crash time are lost by design; `AtLeastOnce` events are recovered from the durable log
  (§Recovery Mechanisms).
- **Slow or stuck subscriber.** Escalates through the subscription's own `BackpressurePolicy`
  (§Algorithms); a persistently stuck `Buffer`-class subscriber (e.g., a frozen Workspace) is
  eventually force-disconnected and must resubscribe, while a `Durable`-class subscriber is never
  disconnected for being slow — it may fall behind, but it may not be told to drop history, since
  [01 §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable) requires every autonomous
  action stay auditable.
- **Duplicate delivery under `AtLeastOnce`.** Possible after a bus restart or a redelivery timeout;
  subscribers are required by contract to dedupe on `(topic, seq)`, the same discipline [30 — IPC
  Framework](30-ipc-framework.md#recovery-mechanisms) requires for idempotent call retries.
- **Federated split-brain.** A network partition between bus shards ([21 — Distributed
  Execution](21-distributed-execution.md)) can leave two shards independently assigning `seq` for a
  topic whose subject has moved; reconciliation on heal is last-writer-wins per shard-local `seq`
  for `AtMostOnce` topics and an explicit merge-and-flag for `AtLeastOnce`/audit topics, never a
  silent drop.
- **Event storms.** A mass reconnect (e.g., a whole peripheral bus flapping) is absorbed by
  `Coalesce` for state-class topics and rate-shaped for `Buffer`-class ones, rather than forwarding
  every raw occurrence.

## Recovery Mechanisms

`AtLeastOnce` subscribers resume from `last_acked_seq` via `replay_from`, reading the durable log
[28 — Storage Engine](28-storage-engine.md) maintains — this is the audit/coordination path's
recovery guarantee, and it is deliberately *not* offered to `AtMostOnce` subscribers, who instead
request a fresh current-state snapshot on reconnect (e.g., a Workspace re-renders from the
Knowledge Graph's current object state rather than replaying every intermediate change), consistent
with [33 — Rollback & Recovery](33-rollback-recovery.md)'s general principle that recovery strategy
matches what was actually promised, not a one-size-fits-all replay. Subscription registries
surviving a bus restart are what make this possible: a restarted bus does not need any subscriber
to re-announce itself, only to reconnect its `Notify` channel.

## Performance Analysis

Local dispatch reuses [30 — IPC Framework's](30-ipc-framework.md#performance-analysis)
`ipc_notify` fast path per subscriber, so per-subscription delivery cost is the same small constant
as a point-to-point notify. Fan-out cost is `O(matching subscriptions)` via the topic trie
(§Algorithms), not `O(all subscriptions)`, which keeps publish latency stable as the Knowledge
Graph and subscriber population grow. Coalescing bounds memory for high-frequency topics (Agent
progress ticks, live cursor-style updates) to `O(subscribers)` regardless of publish rate. The
durable/audit path is bounded by [28 — Storage Engine's](28-storage-engine.md) append throughput,
which is the deliberately chosen bottleneck for that class — it is allowed to be slower than the
in-memory path because it is never allowed to drop. Federated topic propagation latency is
dominated by the same network RTT budget [30 — IPC
Framework](30-ipc-framework.md#performance-analysis) establishes for remote routes; concrete
targets are tracked in [36 — Performance Benchmarks](36-performance-benchmarks.md).

## Trade-offs

**Per-topic ordering vs. global total order.** The bus guarantees monotonic `seq` per `Topic`
only, not a single global order across all topics or shards — a global order would require
coordination that does not scale across a federated bus
([21 — Distributed Execution](21-distributed-execution.md)) for a guarantee almost no subscriber
actually needs. This is a deliberate departure from [07 — Context
Propagation](07-context-propagation.md), which does need strict causal ordering, but only within
one task's point-to-point chain — a fundamentally smaller, cheaper ordering domain than a
system-wide broadcast bus.

**At-least-once plus idempotent consumers vs. exactly-once.** Hyperion chose deduplication by
`(topic, seq)` at the consumer over distributed exactly-once delivery, because the coordination
cost of the latter is not justified when the former is cheap and already required for [30 — IPC
Framework's](30-ipc-framework.md) call-retry semantics — one dedup discipline serves both.

**Coalescing sacrifices history for currency.** A `Coalesce` subscriber only ever sees "the current
value," never the intermediate steps — correct for a Workspace, which represents *present* state,
and specifically wrong for an audit trail, which represents *history*; this is why delivery class
is a per-subscription choice rather than a system-wide default.

## Testing Strategy

Backpressure tests attach an artificially slow subscriber to each `BackpressurePolicy` and assert
the documented behavior per class (coalescing collapses, buffering drops-with-warning, durable
never drops, block stalls the producer). Idempotency/dedup tests replay `AtLeastOnce` streams with
induced duplicates and assert consumer-side correctness. Ordering tests assert per-topic
monotonicity under concurrent publishers and explicitly assert the *absence* of a cross-topic
ordering guarantee, so a future change cannot silently add a cost nobody asked for. Load tests grow
the topic trie to Knowledge-Graph scale and assert fan-out latency stays proportional to matching
subscriptions, not total subscriptions. End-to-end tests confirm [13 — Dynamic UI
Runtime](13-dynamic-ui-runtime.md) Workspaces actually live-update from bus events, and that [34 —
Observability & Telemetry](34-observability-telemetry.md)'s audit sink never loses an event under
induced slowness. Security tests attempt to subscribe to topics without a dominating capability and
assert the discovery-denial behavior matches [30 — IPC
Framework](30-ipc-framework.md#security-considerations).

---
*Next: [32 — Update System](32-update-system.md).*
