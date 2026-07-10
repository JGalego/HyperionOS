# IPC Framework

## Purpose

This document specifies Hyperion's inter-process communication framework: the message-passing
layer that every Capability invocation, every Agent-to-Capability call, and every driver
interaction (per [03 — Kernel Architecture](03-kernel-architecture.md)) actually rides on. [03 —
Kernel Architecture](03-kernel-architecture.md) introduces `endpoint_send`/`endpoint_recv` as the
privileged-core IPC primitive and treats it as a given; this document is the deep-dive on what sits
above and around that primitive: capability-scoped channel discovery, the wire protocol,
synchronous call/response and asynchronous one-way messaging, zero-copy bulk transfer, and
transparent extension of a single invocation across process, container, VM, and — uniquely to this
document — remote-device boundaries. It does not redesign the kernel; every mechanism below is
built from primitives [03](03-kernel-architecture.md) already defines (`CapabilityToken`,
`Object::Endpoint`, the sandboxing depth spectrum).

## Motivation

[02 — Core Architecture §5](02-core-architecture.md#5-capability-security-as-the-unifying-security-model)
requires exactly one security model, enforced at the kernel boundary; [03 — Kernel
Architecture](03-kernel-architecture.md) shows this makes IPC — not a syscall table — the *only*
way any code touches anything outside its own address space. IPC is therefore not one subsystem
among many: it is the substrate every other subsystem is built on. Three pressures converge on the
design here:

1. **Every Capability invocation is an IPC round trip.** [04 — Scheduler](04-scheduler.md) budgets
   end-to-end Intent latency for interactive workspace generation; if IPC overhead is not bounded
   and predictable, that budget is unmeetable no matter how fast a Capability's own logic is.
2. **A process must not be able to discover what it cannot use.** [15 — Security
   Architecture](15-security-architecture.md) and [17 — Threat Model](17-threat-model.md) both
   depend on IPC endpoints being unenumerable without a capability, not merely unusable —
   otherwise a compromised process can map the system's shape even while unable to act on it.
3. **An Agent invoking a Capability must not need to know where the implementation runs.** [02 —
   Capability](02-core-architecture.md#capability) states the OS, not the developer, chooses an
   implementation based on context and resources; [21 — Distributed
   Execution](21-distributed-execution.md) extends that choice across federated devices. If the
   caller's code differs by locality, that choice stops being free for the OS to make.

## Architecture

[02 — Core Architecture](02-core-architecture.md#2-shared-vocabulary) defines a **Trust Boundary**
as "a process, container, VM, or remote host." [03 — Kernel
Architecture](03-kernel-architecture.md#sandboxing-as-one-spectrum) fully covers the first three as
one depth-dialed spectrum (0 in-process, 1 process, 2 container, 3 VM), all enforced by the same
local privileged core. This document is the fourth case: a **remote host**, where no single
privileged core is in a position to enforce anything, and every message must carry its own proof of
authority. The IPC Framework is the layer that makes an invocation look identical across all four
cases to the code issuing it.

```
        Agent (11) or Capability invokes another Capability by CapabilityToken (02 §5)
                                        │
                                        ▼
                    ┌──────────────────────────────────────┐
                    │  Typed Stub — generated from the      │
                    │  Capability's 26-apis.md contract,     │
                    │  caller-side, in-process                │
                    └───────────────────┬──────────────────┘
                                        │ ipc_call() / ipc_notify()
                                        ▼
                    ┌──────────────────────────────────────┐
                    │        IPC Runtime  (L1)                │
                    │  routing table: token → Route            │
                    │  (resolved at grant time, §Algorithms)    │
                    └─────────────┬─────────────┬────────────┘
                       Local route │             │ Remote route
                                    ▼             ▼
              ┌───────────────────────┐   ┌─────────────────────────────┐
              │ kernel endpoint_send /  │   │  Federation Gateway           │
              │ endpoint_recv (03) +    │   │  (21 — Distributed Execution) │
              │ region capability for   │   │  authenticated QUIC stream    │
              │ zero-copy bulk transfer │   └──────────────┬────────────────┘
              └───────────┬────────────┘                    │
                          ▼                                   ▼
              ┌───────────────────────┐   ┌─────────────────────────────┐
              │ Target service, this    │   │ Remote Federation Gateway →   │
              │ Trust Boundary, depth    │   │ remote kernel endpoint →       │
              │ 0-3 (03)                │   │ remote service, same schema    │
              └───────────────────────┘   └─────────────────────────────┘
```

The routing table is populated at capability-grant time, not resolved by name at call time: a
`CapabilityToken`'s `origin` field (03 §Data Structures) already distinguishes which Trust Boundary
minted it, so the IPC Runtime knows at `channel_open` whether a token resolves to a local endpoint
or a remote gateway before a single byte moves.

### Capability-scoped discovery

An endpoint is never named by a string a process can guess or enumerate. `channel_open` takes a
`CapabilityToken` the caller must already hold; if the token is absent, malformed, or revoked, the
IPC Runtime returns the identical `Fault::NoSuchCapability` it would return for an endpoint that
does not exist at all. A process without a token cannot distinguish "wrong token" from "nothing is
there" — which is what makes discovery, not just access, capability-gated, the property [15 —
Security Architecture](15-security-architecture.md) and [17 — Threat
Model](17-threat-model.md) require.

## Data Structures

```rust
/// A capability-scoped, schema-typed communication endpoint. Wraps a kernel
/// Endpoint capability (03-kernel-architecture.md) with what's needed to
/// frame, route, and — if the peer is remote — federate a call.
struct Channel {
    endpoint: CapabilityToken,      // kernel Object::Endpoint capability (03)
    schema_id: SchemaId,            // wire schema this channel speaks (26-apis.md contract)
    route: Route,
    class: ChannelClass,
}

enum Route {
    Local,
    Remote { device_id: DeviceId, gateway: CapabilityToken },  // 21-distributed-execution.md
}

enum ChannelClass { Call, Notify }

/// Wire frame: the only byte-level representation that crosses a Trust
/// Boundary. Fixed header, schema-typed body, optional zero-copy attachment.
struct Frame {
    magic: u32,                     // 0x48594950  ("HYIP")
    version: u16,
    schema_id: SchemaId,            // resolves to a compiled schema; never reflected at runtime
    flags: FrameFlags,              // CALL | REPLY | NOTIFY | ERROR | HAS_REGION
    request_id: u64,                // correlates CALL <-> REPLY; 0 for NOTIFY
    payload_len: u32,
    region: Option<RegionDescriptor>, // present iff HAS_REGION
    payload: Vec<u8>,                // schema-encoded control fields
}

/// A capability to a shared memory region, minted by the kernel exactly like
/// any other object capability (03 §Data Structures). Grants MAP rights over
/// a physical range without copying its bytes into the frame.
struct RegionDescriptor {
    region_cap: CapabilityToken,
    offset: u64,
    len: u64,
}

/// One entry in a Trust Boundary's channel routing table, resolved at grant
/// time (see Architecture, above).
struct RoutingEntry {
    token: CapabilityToken,
    route: Route,
    latency_class: LatencyClass,    // consumed by 04-scheduler.md
}
```

## Algorithms

**Synchronous call/response (rendezvous).** `ipc_call` opens or reuses a `Channel`, constructs a
`CALL` frame with a fresh `request_id`, and — local case — invokes `endpoint_send` then blocks on
`endpoint_recv` for the matching `REPLY`; remote case — hands the frame to the Federation Gateway,
which forwards it over an already-authenticated QUIC stream and returns the correlated reply frame
when it arrives. The caller-side stub is identical in both cases; only the `Route` variant resolved
at `channel_open` differs.

**Asynchronous one-way notify.** `ipc_notify` constructs a `NOTIFY` frame (no `request_id`
correlation, no blocking wait) and returns as soon as the frame is enqueued at the peer's endpoint.
This is the point-to-point sibling of the broadcast mechanism in [31 — Event
System](31-event-system.md): `ipc_notify` is 1:1, sender-knows-receiver, used for things like a
Capability telling its one calling Agent "I'm 40% done," where [31](31-event-system.md) is 1:N and
receiver-unknown-to-sender.

**Zero-copy bulk transfer.** For payloads above a threshold set from [04 — Scheduler's](04-scheduler.md)
latency budget (large Semantic Object blobs per [09 — Knowledge Graph](09-knowledge-graph.md) and
[28 — Storage Engine](28-storage-engine.md)), the sender calls `region_share` to mint a
`RegionDescriptor` over its own buffer rather than serializing it into `payload`. Only the
descriptor — a few dozen bytes — crosses the synchronous path; the receiver calls `region_map` to
map the same physical pages (read-only, or copy-on-write for mutation) into its own address space.
This mirrors the shared-memory fast path [03 — Kernel
Architecture](03-kernel-architecture.md#why-a-pure-microkernel-is-viable-if-its-cost-is-paid-down)
already commits to for driver I/O; the IPC Framework extends it to any Capability payload, not just
block/network/GPU buffers.

**Transparent remote resolution.** At `channel_open`, the runtime inspects the token's `origin` (03
§Data Structures). If `origin` names this Trust Boundary or a local descendant, `route` is `Local`.
If it names a federated device's Trust Boundary, `route` is `Remote`, and the runtime resolves —
from a capability-scoped directory object, never a free-text lookup — which Federation Gateway
endpoint reaches it. The resolved `Route` is cached on the `Channel`; the caller's stub code,
generated once from the Capability's [26 — APIs](26-apis.md) contract, never branches on it.

## Interfaces / APIs

```
channel_open(token: CapabilityToken, schema: SchemaId) -> Result<Channel, IpcFault>
channel_bind(token: CapabilityToken, schema: SchemaId, handler: ServiceFn) -> Result<(), IpcFault>
ipc_call(chan: &Channel, req: Request, timeout: Duration) -> Result<Response, IpcFault>
ipc_notify(chan: &Channel, note: Notification) -> Result<(), IpcFault>
region_share(buf: &[u8], rights: RightsMask) -> RegionDescriptor
region_map(desc: RegionDescriptor) -> Result<MappedRegion, Fault>
```

`ipc_call` and `ipc_notify` are implemented entirely in terms of `endpoint_send` / `endpoint_recv`
(03 §Interfaces / APIs) plus, for `HAS_REGION` frames, `region_share`/`region_map`; there is no
second, parallel syscall surface — only a typed, framed convention layered over the kernel's two
primitives.

## Pseudocode

```rust
fn ipc_call(chan: &Channel, req: Request, timeout: Duration) -> Result<Response, IpcFault> {
    let request_id = next_request_id();
    let (payload, region) = encode(&req, chan.schema_id)?;   // schema-based, no reflection
    let frame = Frame {
        magic: HYIP_MAGIC, version: WIRE_VERSION, schema_id: chan.schema_id,
        flags: FrameFlags::CALL | region.as_ref().map_or(FrameFlags::NONE, |_| FrameFlags::HAS_REGION),
        request_id, payload_len: payload.len() as u32, region, payload,
    };

    match &chan.route {
        Route::Local => {
            endpoint_send(chan.endpoint, frame.into())?;                 // 03: kernel fast path
            let reply = with_deadline(timeout, || endpoint_recv(chan.endpoint))?;
            decode_reply(reply, request_id)
        }
        Route::Remote { gateway, .. } => {
            // Same frame, same schema; the gateway is just another capability-
            // scoped endpoint (21-distributed-execution.md), not a special case.
            let reply = with_deadline(timeout, || federation_send_recv(*gateway, frame))
                .map_err(IpcFault::from_remote)?;
            decode_reply(reply, request_id)
        }
    }
}

fn ipc_notify(chan: &Channel, note: Notification) -> Result<(), IpcFault> {
    let (payload, region) = encode(&note, chan.schema_id)?;
    let frame = Frame {
        magic: HYIP_MAGIC, version: WIRE_VERSION, schema_id: chan.schema_id,
        flags: FrameFlags::NOTIFY, request_id: 0,
        payload_len: payload.len() as u32, region, payload,
    };
    match &chan.route {
        Route::Local  => endpoint_send(chan.endpoint, frame.into()).map_err(IpcFault::Kernel),
        Route::Remote { gateway, .. } => federation_send(*gateway, frame).map_err(IpcFault::from_remote),
    }
}
```

## Security Considerations

Discovery denial (§Architecture) closes the reconnaissance path a compromised process would
otherwise use to map live endpoints. Every frame still carries the caller's own capability token in
its schema-encoded fields, not merely relies on channel identity, which preserves [03's
confused-deputy prevention](03-kernel-architecture.md#security-considerations) at the framing
layer: a server that receives a call is handed the caller's authority explicitly, never
substituting its own ambient rights. Remote routes are additionally protected by the Federation
Gateway's mutual authentication (device identity plus a capability minted specifically for that
device pairing, per [21 — Distributed Execution](21-distributed-execution.md)) and transport
encryption; a stolen frame in transit is useless without both the channel's schema and a live,
unexpired token, and a replayed frame is rejected because the token's `generation` (03 §Data
Structures) is re-validated at the receiving end exactly as for a local `cap_invoke`. Per-token
channel-open rate limits (set by [04 — Scheduler](04-scheduler.md) resource ledgers) bound
endpoint-exhaustion denial-of-service. All channel opens and remote federation calls are logged for
[18 — Explainability & Trust](18-explainability-and-trust.md) and [34 — Observability &
Telemetry](34-observability-telemetry.md); full threat coverage is in [17 — Threat
Model](17-threat-model.md).

## Failure Modes

- **Peer crash mid-call.** Identical to [03's](03-kernel-architecture.md#failure-modes) driver
  crash case: the peer's capability generation bumps, and the caller's pending `endpoint_recv` or
  federation wait returns `Fault::Revoked` rather than hanging.
- **Network partition on a remote route.** The Federation Gateway surfaces
  `IpcFault::PeerUnreachable` after its own timeout; per [02 §4's](02-core-architecture.md#4-design-invariants)
  "degrade, never fail closed," the caller's Capability abstraction ([02 —
  Capability](02-core-architecture.md#capability)) substitutes a local or cached implementation
  where one exists, rather than propagating a bare failure to the Agent.
- **Zero-copy region fault.** A stale or out-of-range region access faults the IOMMU/MMU exactly as
  in [03](03-kernel-architecture.md#failure-modes) and is contained identically, not silently
  corrupting the receiver.
- **Schema/version mismatch.** Detected at `channel_open` (schema negotiation, §Trade-offs) or, for
  a frame that slips through, at decode time, returning `IpcFault::SchemaMismatch` rather than
  misinterpreting bytes.
- **IPC deadlock / priority inversion.** A synchronous call chain (A calls B calls A) or a
  low-priority server blocking a high-priority caller is addressed jointly with [04 —
  Scheduler](04-scheduler.md)'s priority inheritance, over the same rendezvous primitive [03 —
  Kernel Architecture](03-kernel-architecture.md#failure-modes) flags as a shared concern; this
  document supplies the hook (`latency_class` on `RoutingEntry`) the scheduler reads to act on it.

## Recovery Mechanisms

Local peers are restarted under the same supervisor-tree "microreboot" pattern [03 — Kernel
Architecture](03-kernel-architecture.md#recovery-mechanisms) describes; a caller simply re-opens
its `Channel` against a freshly minted capability, since capability tables — not IPC state — are
the sole source of truth. Calls whose Capability contract ([26 — APIs](26-apis.md)) declares
idempotency are safely retried by `request_id` deduplication at the server; calls that are not
idempotent surface the failure to the calling Agent for an explicit retry/undo decision, per [33 —
Rollback & Recovery](33-rollback-recovery.md). Federation Gateways maintain exponential-backoff
reconnection and, for `NOTIFY`-class traffic only, a bounded local queue so a brief partition does
not silently drop bulk state — bounded specifically so it cannot become an unbounded memory leak
during a long partition, at which point the oldest queued notifications are dropped with the same
"coalesce, don't accumulate" logic detailed in [31 — Event System](31-event-system.md).

## Performance Analysis

Local same-core call/response targets the same order of magnitude [03 — Kernel
Architecture](03-kernel-architecture.md#performance-analysis) budgets for the underlying
rendezvous (sub-microsecond to low-single-digit-microsecond), since framing and schema
encode/decode use precomputed schema descriptors rather than runtime reflection — the cost added
above the bare kernel primitive is a fixed, small constant, not a parsing pass. Zero-copy bulk
transfer is bounded by memory bandwidth, not payload size, because only the `RegionDescriptor` —
not the Semantic Object blob itself — crosses the synchronous path; this is what keeps
large-document Capability invocations inside [04 — Scheduler's](04-scheduler.md) interactive
latency budget instead of scaling with file size. Remote-route latency is dominated by network RTT,
not framework overhead: a same-LAN federated call is budgeted in the low single-digit
milliseconds, an order of magnitude above the local path, which is why [21 — Distributed
Execution](21-distributed-execution.md)'s placement decisions treat "local vs. federated" as a
real cost, not a free abstraction, even though the calling code is oblivious to it. Concrete
targets are tracked against [36 — Performance Benchmarks](36-performance-benchmarks.md).

## Trade-offs

**Schema-based binary framing vs. a self-describing format (JSON/CBOR).** Hyperion accepts the
engineering cost of schema distribution and versioning — every `SchemaId` must be resolvable by
both ends, coordinated with [26 — APIs](26-apis.md) contracts and versioned like [29 — Database
Schema](29-database-schema.md) migrations — in exchange for encode/decode costs that don't scale
with reflection, which the performance budget above requires.

**Transparent location independence vs. explicit locality awareness.** Making local and federated
calls look identical to the caller serves the Golden Rule
([01 §2](01-vision-and-philosophy.md#2-the-golden-rule)) by letting an Agent developer, and the
OS's own placement logic, ignore locality — but it means a naive Capability author could
unknowingly write a hot loop of calls that happen to resolve remotely. Hyperion resolves this by
exposing an optional `latency_class` hint on `RoutingEntry` (§Data Structures) that
performance-sensitive Capabilities may query without breaking the default transparency for
everyone else.

**Capability-gated discovery vs. legitimate service browsing.** Denying discovery by default
(§Architecture) means there is no "list nearby printers" call to reach for; Hyperion instead models
a browsable scope as its own capability-scoped **directory object**, so browsing is itself an
audited grant rather than a carved-out exception to the discovery-denial rule.

## Testing Strategy

The frame parser is fuzzed continuously (malformed headers, truncated payloads, capability
substitution inside encoded fields) independently of the kernel's own IPC surface testing (03
§Testing Strategy), since it is new attack surface this document introduces. Zero-copy region
lifetime is checked with property tests asserting no receiver can observe a region after its
capability's generation has been bumped (use-after-revoke). Chaos tests kill a peer mid-call and
mid-notify and assert the documented Failure Modes, not hangs. A conformance suite runs the
identical Capability-contract test corpus over a loopback local route and a real (or emulated)
federated route and asserts byte-for-byte schema equivalence — this is the actual test of the
transparency guarantee, not just a unit test of `ipc_call`. Latency regression tests run
continuously against the budgets in [36 — Performance Benchmarks](36-performance-benchmarks.md),
and security tests specifically attempt token forgery and replay across both local and remote
routes, feeding results to [17 — Threat Model](17-threat-model.md).

---
*Next: [31 — Event System](31-event-system.md).*
