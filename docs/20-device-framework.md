# Device Framework

This document defines the **Device Framework**, the L1 System Runtime component (see the layer
diagram in [02 — Core Architecture](02-core-architecture.md#1-layered-system-view)) that treats
every connected device — monitors, phones, cars, robots, wearables, smart-home appliances, AR
glasses, printers, IoT sensors — as a single intelligent environment rather than a collection of
siloed, app-specific peripherals. Every device becomes a [Semantic
Object](02-core-architecture.md#semantic-object) with a capability manifest describing what it
can render, sense, or actuate, built atop the [Kernel's driver
model](03-kernel-architecture.md#driver-model) and exposed upward so [Workspaces](02-core-architecture.md#workspace)
can span it and the [Scheduler](04-scheduler.md) can allocate it.

## 1. Purpose

Give Hyperion one uniform model for "everything the user owns that can render, sense, or act,"
so that a Workspace generated for an Intent is never limited to the screen it was opened on, and
so that a robot's actuator, a phone's camera, and a laptop's GPU are all first-class,
capability-secured, schedulable resources rather than three unrelated integration problems.

## 2. Motivation

The exam-prep example threaded through this specification — "help me study for tomorrow's
exam" — needs notes open on a phone and a countdown timer visible on a kitchen smart display at
the same time, both bound to the same Intent, both updating from the same
[Context Bundle](02-core-architecture.md#context-bundle). A traditional OS treats the phone and
the display as two unrelated devices running two unrelated apps with no shared state. Per the
[Golden Rule](01-vision-and-philosophy.md#2-the-golden-rule), this is a failure: the human's goal
does not care which physical box is nearest to them. The Device Framework exists so that
[13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md) can treat "which surfaces are available right
now" as a query, not a hard-coded assumption, and so that adding a new device class (a robot
vacuum, a car) is a manifest-registration problem, not a new integration for every Agent that
might want to use it.

This also extends [02's design invariants](02-core-architecture.md#4-design-invariants) into the
physical world: **no silent authority** means a device must be granted trust before it receives a
Context Bundle or a Capability invocation; **everything is undoable or versioned** means physical
actuation needs the same reversibility discipline as a file edit, wherever physically possible;
**degrade, never fail closed** means a car losing connectivity mid-navigation hands off to a
phone rather than aborting the Intent.

## 3. Architecture

```
 L6  Experience Layer     Workspace spanning multiple Device Surfaces
                              (phone: notes · smart display: timer)
                                  │ queries available render/sense/actuate surfaces
                                  ▼
 L4  Cognition Layer      Agent / Scheduler resource requests reference
                              Device Capabilities as schedulable resources
                                  │
 L3  Knowledge Layer      Device Semantic Object in Knowledge Graph
                              (capability manifest, presence, trust state,
                               owner, location, related Workspaces)
                                  ▲
 ─────────────────────────────────┴──────────────────────────────────────
 L1  System Runtime       Device Framework
                          ┌─────────────────────────────────────────────┐
                          │ Device Registry & Manifest Store              │
                          │ Discovery Service (mDNS / BLE / Matter /      │
                          │   cloud relay for remote devices)             │
                          │ Pairing & Trust Negotiation                   │
                          │ Presence / Heartbeat Tracker                  │
                          │ Device Capability Broker ──► Scheduler (04)   │
                          └───────────────────┬─────────────────────────┘
                                               │ per-device driver session
                                               ▼
 L0  Kernel               HAL / Driver Model (sandboxed, one Trust
                              Boundary per device)
                          Physical transport: Wi-Fi, BLE, USB, Matter,
                              5G/cellular — conventional, unmodified
```

The Device Framework does not reinvent transport or pairing cryptography any more than
[19 — Networking Stack](19-networking-stack.md) reinvents TCP/IP: BLE bonding, Matter
commissioning, and Wi-Fi association happen exactly as they do on any modern platform. What
Hyperion adds is the layer above that: a uniform Semantic Object representation of the paired
device, and a uniform Capability-secured path from Agent/Workspace intent down to actuator, so
that every device class is reachable through the same broker instead of a bespoke per-vendor SDK
integration at every layer above L0.

## 4. Data Structures

| Structure | Fields | Purpose |
|---|---|---|
| `DeviceObject` (a Semantic Object subtype) | `device_id`, `type` (Display, Mobile, Vehicle, Robot, Wearable, HomeAppliance, Peripheral, Sensor), `manufacturer`, `model`, `capability_manifest`, `trust_state`, `presence_state`, `owner_binding`, `location_context`, `power_profile` | The durable Knowledge Graph node representing the device; queried by Workspaces and the Scheduler. |
| `CapabilityManifest` entry | `capability_name`, `direction` (render \| sense \| actuate), `contract` (input/output schema), `resource_cost_profile`, `latency_class`, `safety_class` | Declares what the device can do; `safety_class` gates actuators (a robot arm) into a higher trust tier than a passive sensor. |
| `PairingRecord` | `device_id`, `trust_level`, `granted_capabilities[]`, `expiry`, `revocation_token` | The capability grant issued at pairing time; enforced identically to any other Trust Boundary crossing per [15 — Security Architecture](15-security-architecture.md). |
| `PresenceEvent` | `device_id`, `state` (connected \| degraded \| disconnected), `timestamp`, `last_seen`, `reachability_path` (local LAN, cloud relay, cellular) | Feeds the transient-connectivity handling in §9–§10. |
| `DeviceScheduleEntry` | `device_id`, `capability`, `requested_by` (agent/workspace id), `priority`, `resource_reservation_window` | The unit the Device Capability Broker hands to the [Scheduler](04-scheduler.md), making a robot's actuator or a display's render surface a resource exactly like CPU or GPU time. |

## 5. Algorithms

### 5.1 Discovery

Standard protocols only — mDNS/DNS-SD and BLE advertisement on the local network, Matter
commissioning for smart-home devices, and cloud-relay registration for devices that are not
locally reachable (a car, a remote vacation-home thermostat). Discovery never bypasses a device's
native pairing protocol; it listens for and normalizes what that protocol already advertises.

### 5.2 Manifest Ingestion

A discovered device's advertised capabilities are normalized into a `CapabilityManifest` and a
`DeviceObject` is created or updated in the Knowledge Graph, linked to its owner, its typical
location, and any Workspaces that have previously used it — structurally the same
create-or-merge pattern used for web entities in
[19 — Networking Stack §5.4](19-networking-stack.md#54-entity-resolution-against-the-knowledge-graph),
applied to physical rather than web-sourced entities.

### 5.3 Trust Negotiation / Pairing

A challenge-response handshake issues a `PairingRecord` scoped to a tiered trust level: **view**
(the device may be shown information), **sense** (the device may report sensor data into a
Context Bundle), or **actuate** (the device may be commanded to change the physical world).
Actuation-tier grants require an explicit, higher-friction user confirmation step — this is a
deliberate exception to [Universal Usability](01-vision-and-philosophy.md#5-universal-usability-highest-priority):
per the Golden Rule's tie-breaking role, physical-world safety wins the tension against
frictionless usability for this one step only.

### 5.4 Presence and Heartbeat

Devices are polled or send heartbeats at an interval derived from their `power_profile` — a
mains-powered smart display can be polled frequently; a battery-powered wearable is polled
sparsely to conserve battery, with the framework preferring push notifications over polling where
the transport supports it. A grace period is applied before a missed heartbeat is escalated from
`connected` to `degraded`, to avoid flapping on transient radio noise.

### 5.5 Cross-Device Workspace Assembly

[13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md) queries the Device Registry for render
surfaces matching the active [Context Bundle](02-core-architecture.md#context-bundle) (same
owner, same location, currently `connected` or `degraded`) and assigns Workspace segments to
them — e.g., notes to the phone's screen, a countdown timer to the kitchen smart display — with
each segment carrying only the slice of the Context Bundle relevant to that surface, per the
least-privilege principle in §8.

### 5.6 Transient-Connectivity State Machine

```
        heartbeat ok
   ┌───────────────────────┐
   │                       ▼
┌──────────┐   miss    ┌──────────┐   grace period    ┌──────────────┐
│connected │ ────────► │ degraded │ ─────────────────► │ disconnected │
└──────────┘  timeout  └──────────┘   exceeded          └──────────────┘
   ▲                       │                                   │
   │      heartbeat        │  buffer commands,                 │ reassign capability
   │      resumes          │  reduce Context Bundle             │ to substitute device
   └───────────────────────┘  to essentials only                │ or degrade in place
                                                                  ▼
                                                        ┌──────────────────┐
                                                        │ reconnect: diff & │
                                                        │ replay/reconcile  │
                                                        └──────────────────┘
```

## 6. Interfaces / APIs

| Capability / API | Direction | Contract |
|---|---|---|
| `device.discover(filter)` | Framework-internal / SDK | Returns currently visible `DeviceObject` refs matching type/owner/location filters. |
| `device.pair(device_id, requested_trust_tier)` | User-initiated, via Workspace | Executes §5.3; returns a `PairingRecord` or a denial with reason. |
| `device.capability.invoke(device_id, capability_name, args)` | Agent/Workspace → Device Capability Broker | Generic dispatcher; the specific capability (`display.render`, `robot.arm.move`, `car.navigation.set_destination`) is declared per-device in its manifest and validated against the manifest's `contract` before dispatch. |
| `Workspace.attachDevice(device_id, role)` | Dynamic UI Runtime | Binds a device as a Workspace surface (e.g. `role=secondary_display`); see [13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md). |
| Event `device.presence.changed` | Published on the [Event System](31-event-system.md) | Lets Workspaces and the Scheduler react to a device entering `degraded`/`disconnected` without polling. |
| Event `device.trust.revoked` | Published on the [Event System](31-event-system.md) | Immediate signal to tear down any in-flight Capability invocations against a revoked device. |

## 7. Pseudocode

```python
def handle_cross_device_workspace(intent: Intent, context_bundle: ContextBundle):
    candidate_devices = device_registry.list(
        owner=context_bundle.owner,
        location__near=context_bundle.location,
        presence_state__in=["connected", "degraded"],
    )

    plan = dynamic_ui_runtime.plan_surfaces(intent, candidate_devices)  # e.g. notes -> phone,
                                                                        # timer -> smart display
    for surface in plan.surfaces:
        pairing = pairing_store.get(surface.device_id)
        if pairing is None or not pairing.grants(surface.required_capability):
            request_pairing(surface.device_id, tier=surface.required_tier)  # §5.3, may block on
            continue                                                       # user confirmation

        scoped_bundle = context_bundle.scoped_to(surface.required_fields)  # least privilege, §8
        schedule_entry = DeviceScheduleEntry(
            device_id=surface.device_id,
            capability=surface.capability_name,
            requested_by=intent.id,
            priority=intent.priority,
        )
        scheduler.reserve(schedule_entry)  # device capability is a schedulable resource, per 04

        try:
            device_capability_broker.invoke(
                surface.device_id, surface.capability_name, scoped_bundle
            )
        except DeviceUnreachable:
            handle_transient_disconnect(surface, intent)  # §9-10 state machine


def handle_transient_disconnect(surface, intent):
    presence.mark(surface.device_id, "degraded")
    events.publish("device.presence.changed", surface.device_id, "degraded")

    substitute = device_registry.find_substitute(surface, same_owner=True)
    if substitute:
        reassign_capability(intent, surface, substitute)   # e.g. car -> phone mid-navigation
    else:
        queue_action(surface, ttl=surface.grace_period)     # buffer, retry on reconnect

    audit.log_degradation(surface.device_id, intent.id)
```

## 8. Security Considerations

- **Capability-secured pairing, no implicit trust.** Per
  [03 — Kernel Architecture](03-kernel-architecture.md)'s driver model, a device is inert to
  Hyperion until a `PairingRecord` exists; it cannot receive a Context Bundle or a Capability
  invocation before that, mirroring the single capability-security model in
  [02 §5](02-core-architecture.md#5-capability-security-as-the-unifying-security-model).
- **Tiered trust, not binary trust.** View/sense/actuate tiers mean a device compromised for
  sensing cannot silently escalate to actuation; each tier is a separate grant with its own
  expiry and revocation token.
- **Actuation is never silent.** Per the Human Control invariant in
  [01 §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable), any capability with
  `safety_class` above "cosmetic" (a robot arm, a car's controls, a smart lock) requires an
  explicit, interruptible, observable confirmation step before dispatch — the same rule that
  governs any other autonomous action in Hyperion.
- **Least-privilege Context Bundles per surface.** A printer receiving a print job does not
  receive the full Context Bundle behind the Intent that produced it — only the fields the
  invoked capability's contract declares it needs (§5.5, §7).
- **Device impersonation defense.** Manifests are signed where the device's platform supports
  it; an unsigned or newly-changed manifest triggers re-pairing at the lowest trust tier rather
  than silently inheriting a previous grant, defending against a rogue device claiming a
  previously-trusted identity.
- **Encrypted transport even on the local network.** LAN proximity is never treated as an
  implicit trust signal; pairing and invocation traffic is encrypted regardless of network
  locality, consistent with the threat categories in [17 — Threat Model](17-threat-model.md).
- **Revocation is immediate and audited.** A revoked `PairingRecord` tears down in-flight
  invocations against that device within one heartbeat interval and produces an audit entry per
  [15 — Security Architecture](15-security-architecture.md).
- **AR/wearable-display devices get two dedicated tiers, not the generic `Wearable` default.**
  A device that continuously captures camera/spatial data (AR glasses) declares a `sense`
  capability whose `safety_class` is never lower than a standard camera sensor's, precisely
  because it is normally worn and therefore normally recording — pairing surfaces this
  explicitly rather than treating "worn" as implying "trusted." A device that renders directly
  over the user's vision (an overlay Workspace, per
  [13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md)) declares a `render` capability whose
  `safety_class` is elevated above a passive display's, since an unwanted or malicious overlay is
  a physical-safety hazard (obstructed vision) in a way a phone screen is not; the Device
  Capability Broker (§6 `device.capability.invoke`) refuses to schedule a render onto this class
  of surface without an explicit per-Workspace grant, never a blanket "this device may always
  render."

## 9. Failure Modes

| Failure | Detection | Immediate effect |
|---|---|---|
| Mid-task disconnect (car loses connectivity during navigation) | Missed heartbeat past grace period | State machine transitions to `degraded`→`disconnected`; §10 reassignment triggered |
| Manifest mismatch (device firmware changed capabilities without re-declaring) | Contract validation failure at invocation time | Invocation rejected, device flagged for re-pairing |
| Pairing collision (two Hyperion instances pair the same device) | Conflicting `PairingRecord` writes | Last valid, explicitly-confirmed pairing wins; the other is notified and demoted |
| Power exhaustion mid-render (wearable battery dies) | Heartbeat loss + last-known power_profile trend | Treated as disconnect; Workspace surface reassigned or dropped |
| Network partition splitting a Workspace across devices | Presence divergence between surfaces of the same Workspace | Workspace continues on reachable surfaces; unreachable ones marked stale in place |
| Compromised device attempting capability escalation | Invocation request outside granted tier | Denied at the broker, audited, pairing tier reviewed |
| Driver crash at L0 | Kernel-level fault signal | Device marked `disconnected`; Trust Boundary for that device torn down and rebuilt on recovery |

## 10. Recovery Mechanisms

- **Substitute-device handoff.** The canonical example from the brief: a car loses connectivity
  mid-navigation; the framework reassigns the `car.navigation` capability's Workspace surface to
  the user's phone, which continues turn-by-turn guidance from the last-known Context Bundle
  state, per the "degrade, never fail closed" invariant in
  [02 §4](02-core-architecture.md#4-design-invariants).
- **Command buffering with idempotency.** Actuation commands issued while a device is `degraded`
  are queued with an idempotency key rather than dropped or duplicated on reconnect.
- **Reconciliation on reconnect.** When a device returns to `connected`, its local state (if any
  was cached on-device) is diffed against the current Context Bundle; conflicting edits are
  resolved by the same versioning discipline used for any Semantic Object, per
  [33 — Rollback & Recovery](33-rollback-recovery.md).
- **Pre-actuation checkpoints.** Where the physical action is reversible (a smart-home state
  change), a recovery point is recorded before dispatch so the user can undo it, extending the
  "everything is undoable or versioned" invariant into the physical world.
- **Heartbeat grace period tuning.** Prevents flapping between `connected` and `degraded` from
  causing repeated, disruptive reassignments for a momentarily noisy radio link.
- **Revoke-and-re-pair for suspected compromise.** A device that fails contract validation
  repeatedly or attempts out-of-tier invocations has its `PairingRecord` revoked and must
  re-pair from the lowest trust tier, with the incident surfaced to the user per
  [18 — Explainability & Trust](18-explainability-and-trust.md).

## 11. Performance Analysis

Local-network discovery (mDNS/BLE) resolves in tens to low hundreds of milliseconds; cloud-relay
pairing for remote devices (a car, a vacation-home thermostat) is bounded by WAN round trip and
typically completes in one to a few seconds. Heartbeat overhead is the main steady-state cost and
is deliberately traded against battery life: a mains-powered display can sustain sub-second
presence detection, while a wearable's interval is chosen to keep presence-check power draw a
negligible fraction of its total battery budget. Cross-device Workspace assembly adds one Device
Registry query and, for uncached surfaces, one pairing round trip to Workspace generation
latency; this is why the Registry keeps a warm in-memory index of currently-paired devices rather
than re-querying transport-layer discovery on every Workspace. The Device Capability Broker's
integration with the [Scheduler](04-scheduler.md) bounds concurrent actuator commands (a robot
arm accepts exactly one motion command at a time; a display can accept many concurrent render
regions) via the same resource-profile mechanism used for CPU/GPU scheduling. At scale — dozens
of devices in a household, thousands in an enterprise or fleet deployment — discovery and
heartbeat traffic is the dominant cost; see the degradation strategy for constrained deployments
in [37 — Scalability Roadmap](37-scalability-roadmap.md).

## 12. Trade-offs

- **Local discovery vs. cloud relay.** mDNS/BLE is fast and works offline but is
  local-network-bound; cloud relay reaches a car or a remote device but adds latency and a cloud
  dependency. The framework supports both rather than choosing one, consistent with the
  local-first invariant in [02 §4](02-core-architecture.md#4-design-invariants) — cloud relay is
  an explicit upgrade path, not a silent default.
- **Polling frequency vs. battery life.** More frequent heartbeats give faster disconnect
  detection at direct cost to battery-powered devices; the `power_profile`-derived interval in
  §5.4 is a tunable, not a fixed constant, and is expected to be revisited as device classes are
  added.
- **Actuation friction vs. universal usability.** Requiring explicit confirmation for
  actuation-tier grants is a deliberate exception to Hyperion's usability-first design philosophy
  (["01 §5"](01-vision-and-philosophy.md#5-universal-usability-highest-priority)); the Golden
  Rule resolves this tension in favor of physical-world safety for this one class of action.
- **Self-declared manifests vs. centrally certified ones.** Trusting a device's self-declared
  `CapabilityManifest` is more open and extensible (any vendor can join without a certification
  program) but riskier than a centrally verified registry; this is mitigated with progressive
  trust (§5.3) and sandboxed per-device driver sessions (§3) rather than solved by certification
  alone.
- **One shared Knowledge Graph device model vs. per-household partitioning.** Representing every
  device as a Semantic Object in the same graph structure that stores documents and knowledge
  makes cross-device Workspace queries uniform, but requires the same tenant/owner partitioning
  discipline as any other private Semantic Object, per
  [16 — Privacy Architecture](16-privacy-architecture.md).

## 13. Testing Strategy

- **Manifest conformance suite**: a device must pass a behavioral test proving its declared
  capabilities actually work as contracted before its manifest is trusted at anything above the
  lowest tier.
- **Pairing protocol security testing**: replay-attack and MITM fuzzing against the trust
  negotiation handshake in §5.3.
- **Chaos testing for transient connectivity**: kill network mid-actuation and mid-render and
  verify the state machine in §5.6 and the recovery mechanisms in §10 actually trigger, including
  the car-to-phone handoff scenario named in the brief.
- **Cross-device Workspace integration tests**: assert that the exam-prep example (notes on
  phone, timer on smart display, both bound to one Intent) actually splits and stays
  synchronized across both surfaces.
- **Scale testing**: simulate hundreds to thousands of devices to validate discovery and
  heartbeat load against the targets in
  [36 — Performance Benchmarks](36-performance-benchmarks.md).
- **Accessibility conformance**: every cross-device Workspace output must still satisfy
  [14 — Accessibility](14-accessibility.md) constraints on each individual surface, not just the
  primary one.

---
*Next: [21 — Distributed Execution](21-distributed-execution.md).*
