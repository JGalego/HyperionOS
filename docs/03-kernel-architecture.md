# Kernel Architecture

## Purpose

This document specifies Hyperion's L0 Kernel — the only layer in the stack defined in
[02 — Core Architecture](02-core-architecture.md#1-layered-system-view) that runs with hardware
privilege. It covers the hardware abstraction layer (HAL), the driver model, capability security
as the kernel's central primitive, the process/container/VM sandboxing spectrum, and how
virtualization and container runtimes plug into the same capability model. GPU scheduling and AI
inference scheduling are introduced here as first-class kernel scheduling classes but are detailed
in [04 — Scheduler](04-scheduler.md); IPC is introduced here as a kernel-provided primitive but
detailed in [30 — IPC Framework](30-ipc-framework.md). This document is the authoritative source
for the kernel-level enforcement point referenced by [02 §5](02-core-architecture.md#5-capability-security-as-the-unifying-security-model)
and consumed by [15 — Security Architecture](15-security-architecture.md).

## Motivation

[01 — Vision & Philosophy](01-vision-and-philosophy.md) requires Hyperion to run from Raspberry
Pi-class single-board computers to enterprise GPU clusters, to boot in under five seconds, and to
never let intelligence feel like a tax on responsiveness. Simultaneously,
[02 — Core Architecture §4](02-core-architecture.md#4-design-invariants) requires that *no silent
authority ever crosses a Trust Boundary* and that there is *exactly one security model*, enforced
at the kernel boundary and re-checked above it. These two pressures — extreme scalability and
absolute, uniform, kernel-anchored security — are what select the kernel architecture. A design
that satisfies only one of them is disqualified before performance is even considered.

## Architecture

### Why a pure monolith is disqualified

A monolithic kernel (Linux-style: drivers, filesystems, and network stacks linked into one
privileged address space) cannot host the Trust Boundary invariant. Code running in kernel space
has *ambient authority* — it can touch any physical page, any device register, any other driver's
state — by construction, not by grant. A capability token (§Data Structures, below) is only
meaningful if the code holding it is denied everything the token doesn't name; inside a monolith,
every driver already has everything. A single vulnerable Bluetooth driver or filesystem parser
compromises the entire trust base, which means the "exactly one security model" invariant in
[02 §5](02-core-architecture.md#5-capability-security-as-the-unifying-security-model) becomes two
models in practice: capability security for user space, and blind trust for everything linked into
the kernel. Hyperion disqualifies this architecture outright rather than accepting it as a
trade-off, because the invariant it violates is load-bearing for every layer above L0.

### Why a pure microkernel is viable, if its cost is paid down

A pure microkernel (seL4/L4-style: only address-space, thread, and IPC primitives run privileged;
everything else, including memory management policy, is a user-space server) satisfies the Trust
Boundary invariant directly — every driver, filesystem, and network stack is itself capability-
scoped and revocable. Its historical cost is IPC latency: every driver interaction that would be a
function call in a monolith becomes a context switch and a message copy. Hyperion pays this cost
down by design rather than by accepting slower I/O as a permanent tax:

1. **Endpoint capabilities carry data, not just names**, so common calls (read, write, ioctl-class
   requests) complete in one send/receive rendezvous with no additional lookup — detailed in
   [30 — IPC Framework](30-ipc-framework.md).
2. **Shared-memory fast paths** for bulk transfer (block I/O, GPU command buffers, network
   payloads): the kernel grants a capability to a memory *region*, and only the small control
   message ("here is 4 MiB of buffer, go") crosses the synchronous IPC path.
3. **Batched, asynchronous ring-buffer submission** for high-frequency operations (storage queues,
   network packets, GPU/inference dispatch) — modeled on io_uring — so throughput-bound work
   amortizes the per-message cost across a batch instead of paying it per operation.
4. **Core-affine co-scheduling** of a client and the server it talks to most, so an IPC round trip
   is frequently a same-core, same-cache-line handoff rather than a cross-core interrupt; this is
   a scheduling-class decision made jointly with [04 — Scheduler](04-scheduler.md).
5. **Selective privilege inlining** for the tiny number of primitives that are on every hot path
   regardless of workload (physical page allocation, thread dispatch, capability lookup) — these
   stay in the privileged core specifically so the fast paths above have something minimal and
   fast to call into.

The result — argued in full in [Performance Analysis](#performance-analysis) — is a **hybrid
microkernel**: a minimal privileged core (address spaces, threads, IPC primitives, the capability
monitor, and the physical scheduling classes in [04 — Scheduler](04-scheduler.md)) with every
driver, filesystem, network stack, and most "traditional OS" logic implemented as unprivileged,
capability-secured user-space services.

```
┌───────────────────────────────────────────────────────────────────────────┐
│                               USER SPACE                                  │
│                                                                             │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌───────────────────┐  │
│  │  Filesystem   │ │  Network     │ │  GPU / NPU   │ │  Container / VM   │  │
│  │  Server       │ │  Stack       │ │  Driver      │ │  Monitor          │  │
│  │  (10-*.md)    │ │  (19-*.md)   │ │  (04-*.md)   │ │  (27-*.md)        │  │
│  └──────┬───────┘ └──────┬───────┘ └──────┬───────┘ └─────────┬─────────┘  │
│         │                │                │                    │           │
│  ┌──────┴──────┐  ┌──────┴──────┐  ┌──────┴──────┐   ┌─────────┴────────┐  │
│  │ Block/NVMe   │  │ NIC / Wi-Fi │  │ Display /   │   │  Guest OS /      │  │
│  │ Driver       │  │ Driver      │  │ Sensor Drv  │   │  Namespaced Proc │  │
│  └──────┬───────┘  └──────┬──────┘  └──────┬──────┘   └─────────┬────────┘  │
│         │                 │                │                     │          │
│ ────────┴─────────────────┴────────────────┴─────────────────────┴───────  │
│                     capability-secured IPC (30-ipc-framework.md)          │
│ ─────────────────────────────────────────────────────────────────────────  │
│                                                                             │
│                        PRIVILEGED CORE  (L0, this doc)                    │
│  ┌─────────────┐ ┌──────────────┐ ┌───────────────┐ ┌───────────────────┐  │
│  │ Address     │ │ Thread /     │ │ Capability    │ │ HAL device-class  │  │
│  │ Space Mgmt  │ │ IPC Primitive│ │ Monitor        │ │ registry           │  │
│  └─────────────┘ └──────────────┘ └───────────────┘ └───────────────────┘  │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │ Physical scheduling classes: CPU · GPU · Inference · Real-Time UI     │  │
│  │ (dispatch mechanism only — policy lives in 04-scheduler.md)           │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────────────────────────┘
                                    │
                         MMU · IOMMU · VT-x/AMD-V/ARM-VE
                                    │
                              PHYSICAL HARDWARE
```

### Hardware Abstraction Layer

The HAL abstracts by **device class**, never by per-driver special-casing, because Hyperion must
run identically — architecturally, not performance-wise — from a Raspberry Pi-class SBC to an
enterprise GPU cluster, per [01 §10](01-vision-and-philosophy.md#10-success-criteria). Each class
defines a fixed capability contract that any conforming device implements:

- **Compute** (CPU core, GPU, NPU/TPU, inference accelerator) — exposes queue submission, a
  capacity descriptor (cores/SMs/TOPS), and a preemption model.
- **Storage** (NVMe, eMMC, network block device) — exposes a block/queue interface plus a latency
  and durability class.
- **Network** (Ethernet, Wi-Fi, cellular, mesh radio) — exposes a socket/frame interface plus
  bandwidth and reliability descriptors.
- **HID / Sensor** (touch, camera, microphone, IMU) — exposes an event-stream interface.
- **Display** — exposes a framebuffer/compositor-surface interface.

A device driver registers against a class contract, not a bespoke kernel entry point; the HAL
device-class registry (in the privileged core) matches physical devices to drivers by descriptor,
not by hardcoded table. A datacenter GPU cluster and a Pi's integrated NPU both satisfy the
**Compute** class contract — they differ only in the capacity descriptor they advertise, which is
exactly the number [04 — Scheduler](04-scheduler.md) needs to make placement decisions. This is
what lets the same kernel binary scale across the hardware range in
[37 — Scalability Roadmap](37-scalability-roadmap.md) without per-device kernel forks.

### Driver Model

Every driver is an unprivileged, capability-scoped, hot-pluggable user-space process — never
kernel-linked code. A driver receives, at spawn time, exactly three kinds of capability: (1) an
**MMIO/IOMMU capability** scoping which physical device registers and DMA windows it may touch,
(2) an **IRQ capability** delivering only the interrupts of its own device as IPC notifications,
and (3) **endpoint capabilities** to the specific clients it is permitted to serve. Hot-plug is a
first-class event: the [Device Framework](20-device-framework.md) publishes attach/detach events
on the [Event System](31-event-system.md); the kernel's device-class registry mints a fresh
capability set for a newly attached device and revokes it cleanly on detach, without requiring a
reboot or a privileged code path. A driver crash is contained to its own address space and reported
to its device-class registry entry (see [Failure Modes](#failure-modes)).

### Capability Security as the Kernel Primitive

Capability security is not a policy layered onto the kernel — it *is* the kernel's addressing
model. Every reference a process holds to a kernel object (a page, a thread, an endpoint, a device
register range, a VM, a container namespace) is a **capability token**: unforgeable, because it is
only ever constructed by the capability monitor and never guessable or synthesizable by user code;
scoped, because it names one object and one set of rights; revocable, because every token is
reachable from a revocation graph the monitor maintains; and attenuable, because a holder may
derive a strictly narrower token to delegate onward (§Algorithms). This is the single kernel-level
enforcement point that [02 §5](02-core-architecture.md#5-capability-security-as-the-unifying-security-model)
requires and that [15 — Security Architecture](15-security-architecture.md) builds its
process-level, plugin-level, and cross-device policy on top of — the kernel does not know about
"plugins" or "agents," only about tokens, so the security model is genuinely uniform across every
layer in [02 §1](02-core-architecture.md#1-layered-system-view).

### Sandboxing as One Spectrum

Hyperion does not implement "processes," "containers," and "VMs" as three unrelated subsystems
with three permission models. They are one **Trust Boundary depth dial**, backed by the same
capability token format at every depth, differing only in the hardware/software isolation
mechanism that enforces the boundary:

| Depth | Mechanism | Shares with parent | Typical use |
|---|---|---|---|
| 0 — In-process | Language-level (WASM, Rust type safety) | Address space | Trusted plugin logic, hot-path Capabilities |
| 1 — Process | MMU address space, per-process capability table | Kernel ABI, scheduler | Default for drivers, Capabilities, Agents |
| 2 — Container | Namespace + resource-capability isolation | Kernel ABI | Compatibility-layer Linux/Android apps ([27](27-compatibility-layer.md)) |
| 3 — VM | Hardware virtualization (VT-x/AMD-V/ARM-VE), IOMMU-isolated device model | Physical host only | Untrusted or foreign-kernel workloads (Windows, per [27](27-compatibility-layer.md)) |

Choosing a depth is an admission-time decision made from a Capability's declared trust level (see
[02 — Capability](02-core-architecture.md#capability)), not a hardcoded classification — the same
untrusted third-party Capability that runs at depth 1 on a workstation may be promoted to depth 3
on a shared or enterprise deployment. This is why sandboxing is described here as one spectrum: a
single "attenuate and isolate further" operation, parameterized by depth, rather than three
independent code paths to secure and maintain.

### Virtualization and Container Runtime

The container and VM monitors shown in the diagram above are themselves unprivileged servers that
hold **hardware-virtualization capabilities** (EPT/NPT page tables, virtual interrupt injection,
virtio-class device emulation) minted by the privileged core exactly like any other device
capability. A container is granted a capability-scoped view of kernel namespaces (PID, mount,
network) and a resource ledger entry in [04 — Scheduler](04-scheduler.md); a VM is granted a
capability-scoped set of virtual devices and its own guest-kernel Trust Boundary. Both are
consumed identically by [27 — Compatibility Layer](27-compatibility-layer.md) to host foreign
operating systems: an Android app runtime is typically a depth-2 container (shared Linux-derived
kernel ABI, translated Capability surface); a Windows application runtime is typically a depth-3
VM (foreign kernel, hardware-virtualized device model) — but both present their contained software
to the rest of Hyperion as ordinary capability-scoped services, so the [Scheduler](04-scheduler.md),
the [Security Architecture](15-security-architecture.md), and the [Semantic
Filesystem](10-semantic-filesystem.md) do not need special cases for "this process is actually a
whole guest OS."

## Data Structures

```rust
/// An unforgeable reference to exactly one kernel object plus a rights mask.
/// Only ever constructed or narrowed by the Capability Monitor.
struct CapabilityToken {
    token_id: TokenId,          // opaque, monitor-assigned identity of *this* delegation node;
                                 // distinct from object_id (see generation note below)
    object_id: ObjectId,        // opaque, monitor-assigned; never user-synthesizable
    rights: RightsMask,         // bitmask: READ | WRITE | MAP | EXEC | GRANT | REVOKE | ...
    generation: u64,            // snapshot of token_id's node generation; bumped on revocation
    origin: TrustBoundaryId,    // which boundary this token was minted for
    expiry: Option<Instant>,    // optional TTL, per 02 §5
}

/// Per-Trust-Boundary capability table; the *only* way a process addresses
/// anything, including its own memory.
struct CapabilityTable {
    slots: Vec<Option<CapabilityToken>>,
    boundary: TrustBoundaryId,
    parent: Option<TrustBoundaryId>,   // for attenuation/revocation propagation
}

/// A device-class registration, matched to physical hardware by descriptor.
struct DeviceClassEntry {
    class: DeviceClass,                // Compute | Storage | Network | HID | Display | Sensor
    capacity: CapacityDescriptor,       // consumed by 04-scheduler.md for placement
    driver_endpoint: CapabilityToken,   // endpoint capability to the user-space driver
    depth: TrustDepth,                  // 0..3, per the sandboxing spectrum above
}

/// Revocation graph node: every derived token is a child of the token it was
/// attenuated from, so revoking a parent revokes the whole delegated subtree.
struct RevocationNode {
    token: CapabilityToken,
    children: Vec<RevocationNode>,
}
```

## Algorithms

**Capability derivation (delegation with attenuation).** To delegate authority, a holder never
copies its token; it asks the monitor to mint a child token that is a strict subset of its rights,
scoped to the same or a narrower object, optionally with a shorter expiry. The monitor inserts the
child into the revocation graph beneath the parent.

**Revocation.** Revoking a token increments the generation counter on its `RevocationNode` and
recursively invalidates every descendant in one monitor-side graph walk (`O(k)` in the number of
outstanding delegations, not in overall token count) — a single user action ("stop that Agent")
provably removes every downstream authority it had delegated, satisfying [02 §4's](02-core-architecture.md#4-design-invariants)
"everything is undoable" invariant at the kernel layer.

Generation is tracked **per `RevocationNode` (i.e. per `token_id`), not per `object_id`.** A single
object commonly has many independent tokens referencing it — a root capability plus several
attenuated children handed to unrelated holders, each its own node in the revocation graph. Keying
the generation counter by `object_id` instead would mean revoking any one delegated token bumps a
counter shared by every other token for that object, invalidating siblings and even the parent that
performed the revocation — directly contradicting this section's own claim that revocation cascades
only to "the whole delegated subtree," not to the object's entire holder set. The check in
`cap_invoke` below reads `registry_generation(live.token_id)` for this reason.

**Admission at a Trust Boundary depth.** When a Capability is instantiated
(see [02 — Capability](02-core-architecture.md#capability)), the monitor reads its declared trust
level and resource profile, selects a minimum sandboxing depth from the table above, and mints the
capability set for that depth; [15 — Security Architecture](15-security-architecture.md) may
require a deeper boundary than the minimum based on device policy.

## Interfaces / APIs

```
cap_derive(parent: CapabilityToken, rights: RightsMask, ttl: Option<Duration>) -> CapabilityToken
cap_revoke(token: CapabilityToken) -> RevocationReceipt
cap_invoke(token: CapabilityToken, op: Operation, args: &[u8]) -> Result<Reply, Fault>
endpoint_send(token: CapabilityToken, msg: Message) -> Result<(), Fault>      // see 30-ipc-framework.md
endpoint_recv(token: CapabilityToken) -> Message                              // see 30-ipc-framework.md
device_claim(class: DeviceClass, descriptor: CapacityDescriptor) -> CapabilityToken
sandbox_create(depth: TrustDepth, profile: ResourceProfile) -> TrustBoundaryId  // ResourceProfile: 04
sandbox_promote(boundary: TrustBoundaryId, depth: TrustDepth) -> TrustBoundaryId
```

`cap_invoke` is the only way any code, at any layer, ever touches a kernel object — there is no
ambient syscall that bypasses it, which is what makes "exactly one security model" true in
practice rather than only in the spec.

## Pseudocode

```rust
// Capability Monitor: the only privileged-core routine that mints or checks tokens.
fn cap_invoke(caller: &CapabilityTable, tok: CapabilityToken, op: Operation, args: &[u8])
    -> Result<Reply, Fault>
{
    let slot = caller.slots.get(tok.slot_index()).ok_or(Fault::NoSuchCapability)?;
    let live = slot.as_ref().ok_or(Fault::Revoked)?;

    // Reject stale generations: revocation is instantaneous from the token's view.
    // Keyed by token_id (this delegation node), NOT object_id — see the note in
    // §Algorithms above on why a shared per-object counter would over-revoke siblings.
    if live.generation != registry_generation(live.token_id) {
        return Err(Fault::Revoked);
    }
    if live.expiry.map_or(false, |t| now() > t) {
        return Err(Fault::Expired);
    }
    if !live.rights.permits(op) {
        return Err(Fault::InsufficientRights);
    }

    // Dispatch to the object's handler. For device objects this hands off to
    // the owning user-space driver via a same-core IPC fast path (30-ipc-framework.md);
    // for core primitives (threads, address spaces) it is handled in-core.
    match resolve_object(live.object_id) {
        Object::Device(driver_ep) => endpoint_call(driver_ep, op, args),
        Object::AddressSpace(asid) => mmu_op(asid, op, args),
        Object::Thread(tid)        => thread_op(tid, op, args),
        Object::Endpoint(ep)       => ipc_op(ep, op, args),
    }
}

fn cap_derive(caller: &mut CapabilityTable, parent: CapabilityToken,
              rights: RightsMask, ttl: Option<Duration>) -> Result<CapabilityToken, Fault>
{
    let parent_node = revocation_graph::lookup(parent.token_id).ok_or(Fault::NoSuchCapability)?;
    if !parent.rights.contains(&rights) {
        return Err(Fault::CannotEscalate);   // attenuation only: subset, never superset
    }
    let child = CapabilityToken {
        token_id: revocation_graph::fresh_token_id(),  // its own node, own counter
        object_id: parent.object_id,
        rights,
        generation: 0,   // baseline of the *new* node, not the parent's counter value —
                         // the two are independent counters under per-node generation tracking
        origin: caller.boundary,
        expiry: min_ttl(parent.expiry, ttl),
    };
    revocation_graph::attach_child(parent_node, child.clone());
    caller.insert(child.clone());
    Ok(child)
}
```

## Security Considerations

The confused-deputy problem is structurally prevented: a server never uses its own ambient
authority on a caller's behalf, only the capability the caller explicitly passed in the message
(`cap_invoke`'s `args` carry tokens, not names or paths). There is no global capability namespace
to leak into — a token is only meaningful inside the table it was minted for, so exfiltrating a
token's bytes to another Trust Boundary yields nothing. Side channels across shared physical
resources (cache-timing, Spectre-class speculation) are addressed by core-scheduling: Trust
Boundaries with different security domains are never scheduled as hyperthread siblings, a
constraint jointly enforced with [04 — Scheduler](04-scheduler.md). Revocation races (a token used
in the instant between derivation and its parent's revocation) are closed by the monitor checking
the *live* revocation graph generation on every invocation, not a cached copy. Full threat coverage
lives in [17 — Threat Model](17-threat-model.md); the policy layer built on these primitives is in
[15 — Security Architecture](15-security-architecture.md).

## Failure Modes

- **Driver crash.** Contained to the driver's own address space; its capabilities' generation is
  bumped, fencing off in-flight callers, who receive `Fault::Revoked` rather than corrupting state.
- **Capability monitor fault.** Fatal by design — the monitor is small enough to be formally
  verified (see [Testing Strategy](#testing-strategy)) and is the one component with no
  containment boundary above it; a fault here triggers full-system fail-safe halt, not silent
  continuation.
- **IOMMU/DMA misconfiguration.** A driver's DMA capability names an address range; hardware
  IOMMU faults on out-of-range access, which the kernel treats identically to a crash.
- **VM/container escape attempt.** Detected as a capability-rights violation at the hypervisor's
  own token boundary and reported to [17 — Threat Model](17-threat-model.md) instrumentation.
- **IPC deadlock or priority inversion** between a client and the server it depends on — addressed
  jointly with [04 — Scheduler](04-scheduler.md) and detailed in
  [30 — IPC Framework](30-ipc-framework.md).

## Recovery Mechanisms

Drivers and most user-space servers run under a supervisor tree (Erlang/OTP-style): a crashed
device driver is restarted from its `DeviceClassEntry` descriptor with a fresh capability set,
without a kernel reboot — this is the "microreboot" pattern, and it is what lets a flaky Wi-Fi
driver crash and recover in milliseconds rather than degrading the whole device. Restart policy and
state-recovery hooks are detailed in [33 — Rollback & Recovery](33-rollback-recovery.md). Because
capability tables are the sole source of authority, a restarted service simply re-requests the
capabilities it needs from the device-class registry — there is no stale kernel state to reconcile.

## Performance Analysis

The hybrid design's IPC fast path targets sub-microsecond same-core round trips (the historical
seL4 benchmark of ~100–200 cycles for a minimal call is the reference point the privileged core is
budgeted against), with bulk-transfer paths bounded by memory bandwidth, not message count, because
of shared-memory region capabilities. Batching amortizes the fixed per-call cost for high-frequency
operations (storage queues, GPU/inference submission) to a small fraction of a monolithic syscall's
cost per unit of work. The net claim is not that hybrid-microkernel IPC is free — it measurably is
not — but that, combined with core-affine co-scheduling, it is bounded and predictable enough to
meet the sub-second workspace generation and near-instant wake targets in
[36 — Performance Benchmarks](36-performance-benchmarks.md), while a monolith's *unbounded* fault
blast radius is not a cost that can be amortized away at any speed.

## Trade-offs

Decomposing every driver, filesystem, and network stack into its own Trust Boundary is more
engineering work up front than linking them into one address space, and it introduces real,
non-zero IPC latency on cold paths that a monolith would serve with a function call. Hyperion
accepts this cost because the alternative — ambient kernel authority — is disqualified by
[02 §4](02-core-architecture.md#4-design-invariants), not merely disfavored by it. The remaining
engineering trade-off is between decomposition granularity (more services = smaller blast radius
but more IPC hops) and throughput; Hyperion resolves this per-subsystem in the layer documents that
follow (e.g., [10 — Semantic Filesystem](10-semantic-filesystem.md),
[19 — Networking Stack](19-networking-stack.md)) rather than fixing one granularity kernel-wide.

## Testing Strategy

The capability monitor, being small, fixed, and safety-critical, is subject to model checking and
where feasible formal proof of its core invariants (no rights escalation via derivation, no
capability forgeable without going through the monitor, revocation reachability) — the same class
of assurance pioneered by seL4. Drivers and servers are fuzzed at their IPC surface (malformed
messages, capability substitution attacks) independently of the monitor, since they are outside the
trusted computing base by design. Fault injection ("kill this driver mid-transfer," "revoke this
token mid-call") is part of continuous integration, verifying the recovery mechanisms above rather
than only the happy path. A hardware conformance matrix spanning Raspberry Pi-class SBCs through
multi-GPU enterprise nodes exercises the HAL device-class contracts identically across the range
required by [37 — Scalability Roadmap](37-scalability-roadmap.md).

---
*Next: [04 — Scheduler](04-scheduler.md).*
