# Distributed Execution

This document specifies how Hyperion turns every device a single user owns — laptop, desktop,
tablet, phone, home server, a home or rented GPU cluster, and edge devices — into one federated
execution surface, plus the narrower, explicitly consented case of bursting to cloud compute. It is
not a second security model, and not a second storage-consistency model: device federation is built
entirely on the capability tokens defined in [03 — Kernel Architecture](03-kernel-architecture.md)
and the storage convergence already specified in
[28 — Storage Engine](28-storage-engine.md#algorithms). This document adds exactly two things
neither of those documents owns: **which device** a given piece of work should run on, and **how an
in-progress Agent session moves** from one device to another.

## Purpose

Two responsibilities, matching the product brief ("Hyperion should seamlessly execute work across
laptop, desktop, tablet, phone, home server, cloud, GPU cluster, edge devices. The scheduler decides
automatically."):

1. **Work placement.** Extend the [Scheduler](04-scheduler.md)'s admission control
   (04 §Algorithms 5, "Offload decision") across every device in the federation, so a Capability
   invocation that does not fit on the local device is never simply rejected — it is placed on
   whichever federated device, or, only with consent, cloud node, can serve it within its deadline.
   [04 — Scheduler](04-scheduler.md) already treats a remote device as "a virtual `ResourceLedger`
   with an added network-latency term folded into the EDF deadline check, so 'offload' is not a
   separate scheduling regime but one more admission candidate." This document is the thing that
   builds, publishes, and keeps that virtual ledger honest.
2. **State migration.** Let an entire [Agent](02-core-architecture.md#agent) session — its bound
   [Intent](02-core-architecture.md#intent), its [Context Bundle](02-core-architecture.md#context-bundle),
   its reasoning state — move from the device it started on to the device the user is now holding,
   reusing the checkpoint/resume primitive [11 — Agent Runtime](11-agent-runtime.md#63-checkpoint--resume)
   already defines, without the user re-explaining anything ("continue on my phone").

Out of scope, by design, to avoid duplicating a neighboring document: which model implementation
runs for a Capability ([23 — Multi-Model Orchestration](23-multi-model-orchestration.md)); how
inference executes once placed on a device ([22 — Local AI Runtime](22-local-ai-runtime.md)); the
wire format a Context Bundle takes when it crosses a boundary
([07 — Context Propagation](07-context-propagation.md)); the CRDT merge algorithm for already-committed
Semantic Objects ([28 — Storage Engine](28-storage-engine.md#algorithms), which explicitly defers
only "which device is authoritative for which in-flight computation" to this document); and the
canonical Device object schema ([20 — Device Framework](20-device-framework.md)), which this document
extends with federation-specific fields rather than redefines.

## Motivation

A device picker is exactly the kind of "how" the [Golden Rule](01-vision-and-philosophy.md#2-the-golden-rule)
disqualifies: no design that makes a human choose a machine before choosing a goal survives contact
with [01 §5's Universal Usability](01-vision-and-philosophy.md#5-universal-usability-highest-priority).
Three concrete pressures make this a hard architectural requirement rather than a convenience feature:

1. **Devices differ by orders of magnitude in every dimension [04 — Scheduler](04-scheduler.md)
   tracks.** A phone's `vram_mb` and `battery_budget_mw` are nothing like a home GPU rig's. A
   scheduler that only ever sees one device's ledger cannot make good on
   [01 §10](01-vision-and-philosophy.md#10-success-criteria)'s promise that Hyperion feels fast
   "from Raspberry Pi-class devices through enterprise clusters" for a *single user's own hardware
   mix*, not just across the product line in the abstract.
2. **04's own admission algorithm already assumes this document exists.** Its `propose_offload_candidate`
   call is explicitly typed to return to `21-distributed-execution.md`. A task that has exhausted
   every local model-tier degradation must have somewhere to go, or [02 §4](02-core-architecture.md#4-design-invariants)'s
   "degrade, never fail closed" becomes an empty promise the moment a task's minimum viable
   `ResourceVector` simply does not fit on the device the user happens to be holding.
3. **Continuity is a usability requirement, not a nicety.** [01 §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable)
   requires autonomous work to be interruptible and observable *by the user*, not by the user's
   current device. Walking from a laptop to the kitchen mid-Intent, phone in hand, must not cost a
   re-explanation, or the Golden Rule is violated by the platform's own architecture rather than by
   any individual Capability.

**A note on the local-first invariant.** [02 §4.3](02-core-architecture.md#4-design-invariants)
reads: "computation and storage prefer the local device; cloud/remote execution is an explicit,
consented upgrade, never a silent fallback." Applied naively to a federation, this could be
misread as requiring consent every time work moves off the device in the user's hand. This document
reads it more precisely: the invariant protects against *leaving compute the user does not own or
control* — a third party's cloud, a rented GPU cluster, someone else's hardware. Placing work on
*another device the same user owns*, inside the same federation trust boundary, is still local in
every sense the invariant cares about (data residency, no third-party visibility, revocable at will)
and requires no additional consent beyond the one-time act of enrolling that device
(§Security Considerations). Only genuinely external compute — the `CloudBurst` device class in
§Data Structures — is the "explicit, consented upgrade" §4.3 is gating.

## Architecture

```
┌───────────────────────────────────────────────────────────────────────────┐
│   User Capability Root (03 — Capability Security) — one identity;         │
│   every federation membership token is cap_derive()'d from it             │
└───────────────────────────────────┬───────────────────────────────────────┘
                                     │ scoped, revocable per-device tokens
        ┌────────────────────────────┼─────────────────────────────┐
        ▼                            ▼                             ▼
┌────────────────┐          ┌────────────────┐            ┌─────────────────────┐
│     Laptop       │          │     Phone       │            │ Home Server /        │
│ ┌──────────────┐ │          │ ┌─────────────┐ │            │ GPU Cluster           │
│ │ Federation    │◀╪══════════╪▶│ Federation  │◀╪════════════╪▶│ Federation Agent    │
│ │ Agent (§5.1)  │ │  19's    │ │ Agent       │ │  19's      │ │ (§5.1)               │
│ ├──────────────┤ │  conv.   │ ├─────────────┤ │  conv.     │ ├──────────────────────┤ │
│ │ Scheduler(04) │ │  transport│ │ Scheduler(04)│ │ transport │ │ Scheduler(04)        │ │
│ │  + ledger pub │ │  (TLS/   │ │  + ledger   │ │  (TLS/    │ │  + ledger pub /       │ │
│ │ Local AI (22) │ │  QUIC,   │ │   pub (§5.3)│ │  QUIC,    │ │   GPU fan-out         │ │
│ │ Storage (28)  │ │  mDNS /  │ │ Local AI(22)│ │  relay)   │ │ Local AI (22)          │ │
│ │  replica      │ │  relay)  │ │ Storage(28) │ │           │ │ Storage (28) replica  │ │
│ └──────┬───────┘ │          │ └──────┬──────┘ │           │ └───────────┬───────────┘ │
└────────┼─────────┘          └────────┼────────┘            └─────────────┼─────────────┘
         │                             │                                   │
         └───────────────┬──────────────┴───────────────┬───────────────────┘
                          ▼                              ▼
        ┌──────────────────────────────┐   ┌────────────────────────────────┐
        │ Ambient Anti-Entropy           │   │ Anchor Lease Service (§5.2)     │
        │ (28's Merkle/WAL sync, verbatim│   │ one authoritative device per    │
        │  — this doc supplies topology  │   │ in-flight Agent/Intent          │
        │  + priority-sync scheduling)   │   └────────────────┬───────────────┘
        └──────────────────────────────┘                    │ gated by
                                                    ┌──────────▼───────────┐
                                                    │ 16 — Privacy          │
                                                    │ Architecture (consent)│
                                                    └──────────┬───────────┘
                                                               ▼
                                                  ┌────────────────────────┐
                                                  │  Cloud Burst Node(s)     │
                                                  │  explicit consent only   │
                                                  └────────────────────────┘
```

Device-to-device transport rides on the same conventional network primitives (TLS 1.3, QUIC, LAN
discovery) that sit beneath [19 — Networking Stack](19-networking-stack.md#3-architecture)'s
Semantic Networking Layer — federation traffic and web-research egress are sibling consumers of that
shared L1/L0 transport, not a client relationship; 19's Semantic Networking Layer is not on the path
between two of a user's own devices. Each device's Federation Agent is a small, always-resident
service (comparable in role to the sandboxed egress process 19 describes) that maintains the
`FederationTopology`, publishes this device's `VirtualResourceLedger` to peers, and answers offload
and migration requests from them.

## Data Structures

```rust
struct DeviceRecord {                     // federation-specific extension of 20's Device object
    device_id: DeviceId,                  // canonical identity, minted by 20-device-framework.md
    device_class: DeviceClass,            // Laptop | Desktop | Tablet | Phone | HomeServer
                                           // | GpuCluster | Edge | CloudBurst
    compute_capacity: CapacityDescriptor, // cores/SMs/TOPS, per 03-kernel-architecture.md's HAL
    battery_pct: Option<f32>,
    thermal_headroom: Option<f32>,        // sourced from 22 — Local AI Runtime's governor telemetry
    trust_tier: FederationTrustTier,      // OwnedPrimary | OwnedSecondary | SharedHousehold | CloudRented
    transport: TransportProfile,          // measured_rtt_ms, measured_bw_kbps, path: LAN|WAN-relay|BLE
    last_heartbeat: Instant,
}

/// Published by a device to its federation peers; consumed by 04-scheduler.md's admission
/// control as one more ResourceLedger, exactly as 04 §Architecture requires.
struct VirtualResourceLedger {
    device_id: DeviceId,
    ledger: ResourceLedger,               // 04-scheduler.md's struct, unmodified
    network_latency_ms: u32,              // folded into 04's EDF deadline check
    published_at: Instant,
    ttl: Duration,                        // stale ledgers are pruned, never trusted past ttl
}

/// The answer to "who is authoritative for this in-flight computation" that
/// 28-storage-engine.md explicitly defers to this document.
struct AnchorLease {
    owner_intent: IntentId,               // 05-intent-engine.md
    owner_agent: AgentId,                 // 11-agent-runtime.md AgentInstance
    holder_device: DeviceId,
    generation: u64,                      // bumped on every re-lease; mirrors 03's token generation
    granted_at: Instant,
    ttl: Duration,
}

/// Named and typed exactly as referenced by 04-scheduler.md's propose_offload_candidate().
struct OffloadDescriptor {
    ticket: Ticket,                        // 04-scheduler.md's Ticket
    request: ResourceVector,               // 04-scheduler.md's struct, unmodified
    deadline: Option<Instant>,
    cap_token: CapabilityToken,            // 03-kernel-architecture.md
    privacy_tier: PrivacyTier,             // 16-privacy-architecture.md; hard filter, see §Algorithms
}

struct MigrationRequest {
    trigger: MigrationTrigger,             // Explicit | ProximitySignal
    agent_instance: AgentId,               // 11-agent-runtime.md
    source_device: DeviceId,
    target_device: DeviceId,
    checkpoint_id: CheckpointId,           // 11's AgentCheckpoint
    context_bundle: ContextBundleRef,      // 06-context-engine.md / 07-context-propagation.md
    urgency: Ambient | Priority,
}
```

## Algorithms

**Federation join and trust.** Adding a device to the federation mints one `CapabilityToken`
(§03) via `cap_derive` from the user's root identity, scoped to `rights = {PUBLISH_LEDGER,
ACCEPT_OFFLOAD, ACCEPT_MIGRATION}` against the new `DeviceRecord`; content-key wrapping for
synced Semantic Objects is handled by `sync.device.enroll` in
[16 — Privacy Architecture](16-privacy-architecture.md#6-interfaces--apis). There is no second
trust ceremony — federation membership *is* a capability grant, revoked exactly like any other
(03 §Algorithms, "Revocation") the moment a device is removed.

**Anchor lease.** Every `AgentInstance` (11) has exactly one `AnchorLease` holder at a time,
defaulting to the device that spawned it. The lease is renewed on the same heartbeat cadence as
ledger publication and is explicitly released during migration (§5.5) or reclaimed by any peer once
its `ttl` lapses without a heartbeat — a lightweight lease, not distributed consensus, because the
stakes are single-user coordination, not multi-tenant correctness (§Trade-offs).

**Virtual ledger publication.** Each device's Federation Agent publishes its
`VirtualResourceLedger` to peers on a heartbeat (default 2 s over LAN, 30 s over a WAN relay),
discounting capacity by the same throttle factor [22 — Local AI Runtime](22-local-ai-runtime.md#algorithms)
already derives from [04 — Scheduler](04-scheduler.md#algorithms)'s thermal/battery governor —
this document does not sample sensors a second time. Before a ledger entry is ever published to a
peer, it is filtered against [16 — Privacy Architecture](16-privacy-architecture.md#5-algorithms)'s
privacy-gated routing: a device or tier the current `PrivacyProfile` forbids never appears as a
candidate at all, rather than being scored and then rejected. This keeps 04's admission control
itself free of any privacy-tier logic — the Scheduler only ever sees ledgers it is actually allowed
to use.

**Task offload execution.** Once 04's admission control selects a remote `VirtualResourceLedger`
(04 §Algorithms 5), it calls `propose_offload_candidate`, which this document resolves into an
`OffloadDescriptor` and dispatches: the minimal Capability input (a Capability reference plus the
scoped Context Bundle slice per [07](07-context-propagation.md)) is shipped to the target; the
target's own [03](03-kernel-architecture.md)/[04](04-scheduler.md)/[22](22-local-ai-runtime.md)
stack admits and executes it under its own Trust Boundary, exactly as it would a local task; the
result streams back and the originating `Ticket` completes on the source device.

**Session/state migration.** "Continue on my phone," whether explicit or triggered by a presence
signal from [20 — Device Framework](20-device-framework.md), executes six steps, five of them
reused wholesale from [11 — Agent Runtime §6.3](11-agent-runtime.md#63-checkpoint--resume):
(1) freeze via `AgentRuntime.checkpoint`; (2) compute a priority-sync manifest of any Context
Bundle entries not yet replicated to the target, per the version generations
[06](06-context-engine.md#data-structures) already stamps on every entry; (3) transfer the
checkpoint and manifest as an urgent sync batch — still an ordinary Object Write Transaction in
[28](28-storage-engine.md#algorithms), just scheduled ahead of ambient anti-entropy rather than
behind it; (4) hand off the `AnchorLease`; (5) resume via `AgentRuntime.resume` on the target,
which re-binds the Intent and re-fetches the Context Bundle exactly as it would after any other
checkpoint restore; (6) terminate the source instance with reason `"migrated"`, which 11's own
audit trail (§5.4) already records without any bespoke migration-logging path.

**Consistency composition.** Durable Semantic Object convergence across devices — the CRDT merge
for graph edges, hybrid-logical-clock last-writer-wins for scalar metadata, and explicit surfacing
of any genuine blob-content fork — is entirely [28 — Storage Engine](28-storage-engine.md#algorithms)'s
algorithm, unmodified. This document's only addition to that picture is the `AnchorLease` layer
above it: 28 guarantees "whichever device computes first, the storage converges"; this document
answers the question 28 explicitly does not, "which device gets to compute first." A conflict at
the federation-orchestration layer (e.g., a lease dispute overlapping a partition) is surfaced as
an event through [31 — Event System](31-event-system.md) and explained through
[18 — Explainability & Trust](18-explainability-and-trust.md) exactly like any storage-layer
conflict 28 detects — this document never introduces a second conflict-resolution surface.

## Interfaces / APIs

```
federation.topology() -> FederationTopology
federation.ledger.publish(ledger: VirtualResourceLedger)                 // heartbeat, device -> peers
federation.lease.acquire(owner_intent, owner_agent, device_id) -> AnchorLease
federation.lease.renew(lease) -> AnchorLease
federation.lease.release(lease)
federation.offload.execute(descriptor: OffloadDescriptor) -> CapabilityResult   // called by 04 post-admission
federation.migrate(agent_instance_id, target_device_id, urgency) -> MigrationReceipt
federation.migrate.cancel(migration_id)                                  // interruptible, per 01 §9
explain(migration_id | offload_ticket) -> DistributionRationale          // for 18-explainability-and-trust.md
```

Events published on [31 — Event System](31-event-system.md): `device.joined`, `device.lost`,
`lease.granted`, `lease.revoked`, `migration.started/completed/failed`,
`offload.dispatched/completed`.

## Pseudocode

```python
def migrate_agent_session(request: MigrationRequest) -> MigrationReceipt:
    lease = lease_table.get(request.agent_instance)
    if lease.holder_device != request.source_device:
        raise NotAuthoritative(request.agent_instance)        # only the current anchor may initiate

    checkpoint_id = agent_runtime.checkpoint(request.agent_instance)     # 11 §6.3 — freeze
    manifest = diff_against_target(request.context_bundle, request.target_device)  # §5, step 2

    receipt = MigrationReceipt(migration_id=new_id(), state="transferring")
    try:
        push_priority_sync(request.target_device, checkpoint_id, manifest,
                            envelope=make_sync_envelope(checkpoint_id, request.target_device))  # 28 + 16
    except (Unreachable, Timeout) as e:
        audit.log(request, outcome="transfer_failed", error=e)
        return receipt.fail(reason=str(e))                    # source keeps checkpoint; see §Recovery

    ack = wait_for_target_ack(request.target_device, checkpoint_id, timeout=MIGRATION_ACK_BUDGET)
    if not ack.verified(expected_hash=checkpoint_hash(checkpoint_id)):
        audit.log(request, outcome="corrupt_transfer")
        return receipt.fail(reason="checksum_mismatch")       # source resumes locally, never stranded

    lease_table.release(lease)
    new_lease = lease_table.acquire(request.agent_instance, request.target_device)

    agent_runtime.terminate(request.agent_instance, reason="migrated")  # 11 §7
    events.publish("migration.completed", request.agent_instance, request.target_device)
    return receipt.succeed(new_lease)


def dispatch_offload(descriptor: OffloadDescriptor) -> CapabilityResult:
    candidates = [l for l in ledger_cache.values()
                  if privacy_gate.allows(descriptor.privacy_tier, l.device_id)      # 16, hard filter
                  and fits(descriptor.request, l.ledger, l.network_latency_ms, descriptor.deadline)]
    if not candidates:
        raise NoFeasiblePlacement(descriptor.ticket)           # bounces back to 04's own retry chain

    target = min(candidates, key=lambda l: placement_cost(l, descriptor))
    try:
        result = remote_invoke(target.device_id, descriptor)   # target's own 03/04/22 stack executes
    except (Unreachable, RemoteAdmissionRefused):
        ledger_cache.invalidate(target.device_id)               # stale ledger; see §Failure Modes
        return dispatch_offload(descriptor)                     # retry against next candidate
    audit.log(descriptor, target.device_id, result.summary)
    return result
```

## Security Considerations

There is exactly one security model in Hyperion, per
[02 §5](02-core-architecture.md#5-capability-security-as-the-unifying-security-model), and this
document introduces no exception to it. A federation membership token is an ordinary
`CapabilityToken` (03), so "remove this device" is the same revocation-graph walk (03 §Algorithms,
`O(k)` in outstanding delegations) that stops a runaway Agent — a compromised or stolen device's
tokens fence off instantly, and its next heartbeat, ledger publication, or lease claim is rejected
by every peer. Migration and priority-sync payloads travel as `SyncEnvelope`s exactly as defined in
[16 — Privacy Architecture](16-privacy-architecture.md#4-data-structures) (per-object AEAD
ciphertext, per-device wrapped content keys) — a rendezvous relay used to reach an off-LAN device
can route the bytes but cannot read them, the same guarantee 16 already makes for routine sync.
Cloud burst nodes are the lowest federation trust tier by default and never receive a published
`VirtualResourceLedger` entry until [16](16-privacy-architecture.md#5-algorithms)'s consent gate has
been consulted for the specific data in scope — an unconsented cloud node is architecturally
invisible to the placement algorithm, not merely deprioritized. `AnchorLease` tokens carry the same
`generation`/`ttl` shape as a `CapabilityToken` specifically to prevent a stale, previously-valid
lease from being replayed into a split-brain window after a network partition heals. Publishing
battery/thermal telemetry to federation peers is minimized to the scalar fields §Algorithms actually
needs (no raw sensor stream), consistent with [16](16-privacy-architecture.md)'s minimal-disclosure
posture. Full adversarial scenarios (a malicious federation peer, a forged ledger, a lease-replay
attack) are catalogued in [17 — Threat Model](17-threat-model.md).

## Failure Modes

- **Target unreachable mid-migration** — the checkpoint transfer never completes or is never
  acknowledged.
- **Anchor split-brain** — clock skew or a missed revocation leaves two devices believing they hold
  the same `AnchorLease`.
- **Compromised or stolen device** still holding valid, unrevoked federation tokens.
- **Stale virtual ledger** — a device slept, throttled, or lost connectivity after publishing
  capacity that no longer reflects reality, and 04 admits a task against it.
- **Cloud burst cost or latency runaway** under sustained offload pressure.
- **Migration to a cold-cache target** — the target device's local Storage Engine replica has not
  yet synced the Semantic Objects the Context Bundle references.
- **Network partition during ambient anti-entropy** — deferred entirely to
  [28](28-storage-engine.md#failure-modes) for the storage-convergence half of the problem; this
  document's concern is only whether an in-flight lease survives the same partition.

## Recovery Mechanisms

An unreachable migration target leaves the source's `AgentInstance` in the `checkpointed` state
[11](11-agent-runtime.md#33-lifecycle-state-machine) already defines; a bounded timeout resumes it
locally rather than stranding it, which is the direct application of
[02 §4](02-core-architecture.md#4-design-invariants)'s "degrade, never fail closed" to migration
specifically. Anchor split-brain is closed by the lease's `generation`/`ttl` fields: on detected
conflict, the deterministic tie-break (higher `FederationTrustTier`, then lower `device_id`) wins,
and the loser discards its in-flight execution and re-checkpoints — safe because 11's checkpoints are
idempotent to discard and no committed storage write has occurred yet (28 owns that boundary). A
stale virtual ledger causes the *target's own* admission control to refuse the task on arrival
(`RemoteAdmissionRefused` in the pseudocode above), which bounces back to the source's normal
retry-and-degrade chain (04 §Algorithms 1) rather than silently dropping the work. A cold-cache
migration target eagerly pre-stages the referenced Semantic Objects as a priority-sync batch before
signaling resume-ready; if the urgency is `Ambient`, the resumed Agent instead proceeds with a
`partial_context` flag surfaced to [18 — Explainability & Trust](18-explainability-and-trust.md)
rather than silently reasoning over an incomplete bundle. Cloud cost/latency runaway is bounded by a
hard per-session cap on concurrent cloud-tier ledger candidates, escalated as a user-visible event
rather than an invisible bill.

## Performance Analysis

Ledger publication adds a small, fixed number of `VirtualResourceLedger` entries to 04's admission
scan per federated peer — 04's own admission cost remains `O(R)` in the fixed number of resource
dimensions (04 §Performance Analysis), not `O(devices)`, because each remote ledger is scored
independently against the same candidate task, not against every other ledger. The heartbeat
interval (2 s LAN / 30 s WAN) bounds how stale a placement decision's inputs can be; a task admitted
against a ledger that went stale in that window fails fast on the target and retries (§Recovery),
which is cheaper than polling every device before every placement decision. Migration latency is
dominated by transfer size, not protocol overhead: an `AgentCheckpoint` (11) plus an incremental
Context Bundle diff typically serializes to well under a few megabytes, which transfers in well
under 200 ms on a LAN and on the order of one to two seconds over a mobile WAN relay — displayed as
a brief, observable "moving to your phone…" transition per
[13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md), never a silent multi-second freeze. Concrete
budgets are tracked in [36 — Performance Benchmarks](36-performance-benchmarks.md).

## Trade-offs

An `AnchorLease` is a lightweight lease, not distributed consensus (Raft/Paxos-class agreement) —
cheaper to implement and reason about, and acceptable because the worst case of a lease dispute is
redoing a small amount of in-flight, not-yet-committed work locally, never data loss, since durable
state remains protected by [28](28-storage-engine.md)'s own atomicity guarantee regardless of which
device held the lease. This document inherits 28's PACELC choice (partition-tolerant and available,
eventually consistent) rather than adding a second, stricter consistency regime just for
orchestration metadata — a stricter regime here would buy nothing, since the data it would protect
is already re-derivable by re-running the Agent. Treating other devices the user owns as local-first
rather than as a separate "upgrade" tier is a deliberate reading of
[02 §4.3](02-core-architecture.md#4-design-invariants) (§Motivation) that trades a small amount of
conservatism (a compromised secondary device is a slightly larger blast radius than a
single-device model) for the continuity experience the product brief requires; per-device trust
tiers (`SharedHousehold` in particular) exist specifically so a user can narrow this when a
federation member is less trusted than their primary device. Finally, relying on 28's ambient
anti-entropy for the bulk of cross-device data and reserving a priority-sync path only for active
migration (rather than building a second, general-purpose real-time replication channel) keeps this
document's transport surface small, at the cost of migration latency being somewhat sensitive to how
much of the Context Bundle has *not* already arrived via ambient sync.

## Testing Strategy

A chaos harness simulates federation-wide network partitions and device churn (devices joining,
sleeping, and leaving mid-task) and asserts every in-flight offload either completes, fails
observably, or is retried against another candidate — never silently dropped. Migration fidelity is
tested by golden-state comparison of `AgentInstance` state before and after a checkpoint/transfer/
resume cycle across simulated LAN, WAN-relay, and degraded-bandwidth conditions, reusing
[11](11-agent-runtime.md#14-testing-strategy)'s own checkpoint-fidelity fixtures extended across a
device boundary. Lease split-brain is deliberately injected (two devices racing to acquire the same
lease under simulated clock skew) to verify the deterministic tie-break and safe discard path.
Virtual-ledger staleness is tested by suspending or throttling a device immediately after it
publishes a ledger and confirming the resulting remote-admission refusal bounces back to the source
rather than stalling. Security regression tests confirm a revoked device's tokens are rejected
federation-wide within one heartbeat interval, and that an unconsented cloud tier never appears as a
placement candidate under any `PrivacyProfile`. These suites extend
[28](28-storage-engine.md#testing-strategy)'s own multi-device chaos tests rather than duplicating
them, and feed the shared harness in [35 — Testing Strategy](35-testing-strategy.md).

---
*Next: [22 — Local AI Runtime](22-local-ai-runtime.md) specifies how a placed invocation actually
executes on whichever device — local or federated — the algorithm above chose for it.*
