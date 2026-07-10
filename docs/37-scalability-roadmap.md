# Scalability Roadmap

## Purpose

This document specifies how the *same* Hyperion architecture — one codebase, one security model,
one scheduler, one set of subsystem contracts — runs on hardware from a Raspberry Pi-class
single-board computer through a laptop, a GPU workstation, and an enterprise cluster serving a
federated fleet of an organization's devices, without forking the codebase per tier. It covers: the
degradation strategy that keeps the targets in [36 — Performance Benchmarks](36-performance-benchmarks.md)
true on constrained hardware by substituting or disabling Capabilities rather than silently
degrading quality; horizontal scale-up for multiple users and organizations, including Knowledge
Graph partitioning ([29 — Database Schema](29-database-schema.md)) and multi-tenant capability
security ([15 — Security Architecture](15-security-architecture.md)); how the federation model in
[21 — Distributed Execution](21-distributed-execution.md) extends from a person's own devices to an
organization's fleet; and a concrete minimum-viable hardware profile with a growth path.

## Motivation

[01 — Vision & Philosophy §10](01-vision-and-philosophy.md#10-success-criteria) requires Hyperion
to scale "from Raspberry Pi-class devices to enterprise clusters" as a single success criterion,
not two products that happen to share a name. [02 §4](02-core-architecture.md#4-design-invariants)'s
fifth invariant — "degrade, never fail closed on the user's goal" — is what makes this possible
without forking: instead of a constrained device running a smaller, differently-architected OS, it
runs the identical layered stack from [02 §1](02-core-architecture.md#1-layered-system-view) with
different `CapacityDescriptor` values ([03 — Kernel Architecture](03-kernel-architecture.md)) and a
different resolved set of Capability implementations. A forked codebase would mean every subsystem
document in this specification effectively describes two systems; a single codebase with
hardware-driven substitution means this specification is complete as written. The commercial and
governance stakes are just as direct: [39 — Commercial Strategy](39-commercial-strategy.md) and
[41 — Implementation Phases](41-implementation-phases.md) both assume Hyperion can ship on
inexpensive hardware and in enterprise deployments from the same release train — that assumption is
only true if this document's degradation and multi-tenancy story holds.

## Architecture

```
                              SAME L0–L6 STACK, EVERY TIER  (02 §1)
        ┌──────────────────────────────────────────────────────────────────────┐
        │  L6 Experience · L5 Coordination · L4 Cognition · L3 Knowledge       │
        │  L2 Platform Services · L1 System Runtime · L0 Kernel                │
        └──────────────────────────────────────────────────────────────────────┘
        Tiers differ only in: CapacityDescriptor (03) · resolved ModelTier mix
        (22/23) · ResourceLedger size (04) · KG partition scope (29/09) ·
        Trust Boundary depth (03) — never in which layers exist.

 TIER 1: SBC          TIER 2: Laptop        TIER 3: Workstation      TIER 4: Enterprise
 (RPi-class)          (consumer default)     (discrete GPU)           Cluster / Fleet
┌───────────┐        ┌───────────┐         ┌───────────┐          ┌─────────────────┐
│ Tiny/Edge  │        │ Small      │         │ Large      │          │ Large + special- │
│ model only │        │ resident + │         │ resident,  │          │ ized models,     │
│ 1 Agent    │        │ Large      │         │ 8+ Agents, │          │ multi-tenant KG  │
│ serialized │        │ on-demand  │         │ federation │          │ partitions,      │
│            │        │ 2-4 Agents │         │ hub device │          │ autoscaled pods  │
└─────┬─────┘        └─────┬─────┘         └─────┬─────┘          └────────┬─────────┘
      │                     │                      │                        │
      └──────────┬──────────┴───────────┬──────────┴────────────┬───────────┘
                  ▼                      ▼                       ▼
     21 — DISTRIBUTED EXECUTION: one federation protocol, two membership scopes
  ┌────────────────────────────────┐        ┌──────────────────────────────────────┐
  │ PersonalMesh: phone+laptop+    │ promote│ OrgFleet: IdP-enrolled devices, org   │
  │ tablet, owner-issued grants,   │───────▶│ capability policy layered on top of   │
  │ mutual capability tokens       │        │ the same personal-grant primitive     │
  └────────────────────────────────┘        └──────────────────────────────────────┘
```

The degradation and partitioning logic described below runs at two decision points: **admission
time**, when a Capability is first installed or a device profile changes (this document), and
**per-task time**, when the [Scheduler](04-scheduler.md) fits a specific task's `ResourceVector`
against current ledger headroom (already specified in [04 §Algorithms 1](04-scheduler.md#algorithms)).
This document's `degrade_capability` is the static, hardware-profile-driven counterpart to that
document's dynamic, per-task-driven admission control; both funnel through the same "cheaper tier,
then alternate implementation, then explicit consent, then explain-and-disable" fallback order.

## Data Structures

```
struct HardwareProfile {
    tier: HardwareTier,                 // SBC | Laptop | Workstation | EnterpriseNode
    compute: CapacityDescriptor,        // 03 — HAL device-class contract (TOPS, cores, VRAM)
    ram_mb: u32,
    storage_class: StorageClass,        // eMMC | NVMe | NetworkBlock
    thermal_envelope_w: u32,
    tenancy: TenancyMode,                // SingleUser | MultiUserShared | MultiTenantOrg
}

struct DegradationPolicy {
    capability_ref: CapabilityRef,        // 02 — Capability
    constraint: ResourceConstraint,        // which HardwareProfile dimension triggers this
    fallback_order: [Substitution],        // ordered candidates, cheapest-preserving-fidelity first
    user_notice: ExplanationTemplate,      // 18 — Explainability & Trust
}

enum Substitution {
    CheaperLocalTier(ModelTier),           // 22/23 — smaller local model, same Capability
    AlternateImplementation(CapabilityRef),// lower-fidelity local implementation
    ConsentedCloudUpgrade(ProviderRef),     // explicit opt-in, never silent (02 §4 invariant 3)
    Disable,                                // last resort; always explained, never silent
}

struct TenantPartition {
    tenant_id: TenantId,                    // org or household
    kg_shard_ref: ShardId,                   // 29 — Database Schema
    cross_partition_edges: [CapabilityGatedEdge],  // traversal requires explicit grant
    resource_ledger_ref: LedgerId,           // 04 — Scheduler, sharded per tenant/NUMA domain
}

struct FederationMembership {
    device_id: DeviceId,
    scope: PersonalMesh | OrgFleet,
    issued_by: IdentityAuthority,             // device owner (personal) or org IdP (fleet)
    trust_depth: TrustDepth,                   // 03 — sandboxing spectrum, 0..3
    grant: CapabilityToken,                    // revocable, per 03's revocation graph
}
```

## Algorithms

**1. Degradation decision.** At Capability install time or on any `HardwareProfile` change
(new device, or a persistent thermal/memory ceiling), `degrade_capability` walks
`fallback_order`: try the same Capability at the next-cheaper local `ModelTier`
([22](22-local-ai-runtime.md)/[23](23-multi-model-orchestration.md)); if none fits, try an
alternate, lower-fidelity local implementation of the same semantic contract
([02 — Capability](02-core-architecture.md#capability)); if none fits, offer a consented cloud
upgrade — never a silent fallback, per [02 §4](02-core-architecture.md#4-design-invariants)
invariant 3; only if the user declines or no network/consent path exists is the Capability
disabled. Every step short of the first is logged and surfaced via `user_notice`
(Interfaces/APIs), satisfying invariant 5 ("degrade, never fail closed") and
[18 — Explainability & Trust](18-explainability-and-trust.md) simultaneously — a user is never
left wondering why a result is thinner than expected.

**2. Knowledge Graph partitioning for scale-up.** Enterprise deployments partition the Knowledge
Graph first by `tenant_id` (an organization or household), then by user/workspace inside that
tenant, per [29 — Database Schema](29-database-schema.md). Sharding keys co-locate a user's own
Semantic Objects for read locality. Traversal across a partition boundary — "org knowledge" an
Agent needs but that lives in another user's or team's shard — requires an explicit
`CapabilityGatedEdge` grant; there is no default-open cross-partition read, mirroring
[03 — Kernel Architecture](03-kernel-architecture.md)'s "no ambient authority" rule at the data
layer instead of the process layer.

**3. Multi-tenant capability security.** Every `CapabilityToken` in a multi-tenant deployment
additionally carries the `TenantId` it was minted for. The capability monitor's admission check
([03 — Kernel Architecture §Algorithms](03-kernel-architecture.md#algorithms)) rejects any
operation whose token `TenantId` does not match the target object's partition — this is a single
extra comparison added to an existing check, not a second, parallel security model, consistent
with [02 §5](02-core-architecture.md#5-capability-security-as-the-unifying-security-model)'s
"exactly one security model" requirement. `TenancyMode::MultiTenantOrg` also raises the minimum
Trust Boundary depth for Agents from the personal-device default (depth 0/1) to depth 2/3
(container/VM, per [03](03-kernel-architecture.md#sandboxing-as-one-spectrum)), so tenants sharing
physical infrastructure never share an address space.

**4. Federation extension from person to fleet.** `FederationMembership` generalizes
[21 — Distributed Execution](21-distributed-execution.md)'s personal-mesh protocol rather than
replacing it: the discovery, mutual-attestation, and capability-grant handshake are identical
whether the peer is the user's own tablet or a colleague's enrolled laptop. The only difference is
who the `issued_by` `IdentityAuthority` is — the device owner for `PersonalMesh`, an
organization's identity provider for `OrgFleet` — and that org-issued grants are always
*attenuations* of what a personal grant could express, never a broader primitive
(mirrors [03 §Algorithms](03-kernel-architecture.md#algorithms)'s "attenuation only, never
escalation" derivation rule). Revocation cascades through the same `O(k)`-in-delegations
revocation-graph walk [03](03-kernel-architecture.md#algorithms) already defines: one admin action
("offboard this device") removes every downstream authority it had, org-wide, in one step.

## Interfaces / APIs

```
hardware_profile_detect() -> HardwareProfile
degrade_capability(cap: CapabilityRef, profile: HardwareProfile) -> DegradationPlan
apply_degradation(plan: DegradationPlan) -> Capability
explain_degradation(plan: DegradationPlan) -> ExplanationReport         // 18
kg_partition_resolve(object: SemanticObjectRef) -> ShardId               // 29
tenant_grant_cross_partition(from: TenantId, to: TenantId, edge: EdgeType) -> CapabilityToken
federation_join(device: DeviceId, scope: PersonalMesh | OrgFleet, authority: IdentityAuthority)
    -> FederationMembership
federation_revoke(device: DeviceId) -> RevocationReceipt                 // cascades per 03
```

## Pseudocode

Scaling-decision algorithm run at Capability install time and on hardware-profile change:

```python
def degrade_capability(cap: CapabilityRef, profile: HardwareProfile) -> DegradationPlan:
    policy = degradation_policies.lookup(cap)
    if policy is None or not policy.constraint.violated_by(profile):
        return DegradationPlan.full_fidelity(cap)          # no constraint hit; nothing to degrade

    for substitution in policy.fallback_order:
        match substitution:
            case CheaperLocalTier(tier):
                candidate = model_router.resolve(cap, tier)          # 23
                if scheduler.would_fit(candidate.resource_vector, profile):    # 04
                    return DegradationPlan(
                        capability=candidate, substitution=substitution,
                        notice=render(policy.user_notice, tier=tier))

            case AlternateImplementation(alt_cap):
                if scheduler.would_fit(alt_cap.resource_vector, profile):
                    return DegradationPlan(
                        capability=alt_cap, substitution=substitution,
                        notice=render(policy.user_notice, alt=alt_cap))

            case ConsentedCloudUpgrade(provider):
                if user.has_consented(provider):                     # 16 — never silent
                    return DegradationPlan(
                        capability=cloud_binding(cap, provider), substitution=substitution,
                        notice=render(policy.user_notice, provider=provider))
                else:
                    ask_user_for_consent(provider, cap)               # explicit, deferred choice

            case Disable:
                pass  # fall through only if every prior candidate failed to fit or was declined

    return DegradationPlan.disabled(
        capability=cap,
        notice=render(policy.user_notice, reason="no fitting implementation on this device"))


def apply_and_explain(plan: DegradationPlan):
    installed = capability_registry.install(plan.capability)          # 24 — Plugin Framework
    audit_log.append(plan.notice, capability=plan.capability, substitution=plan.substitution)  # 18
    ui.surface_dismissible_notice(plan.notice)                        # 13 — Dynamic UI Runtime
    return installed
```

## Security Considerations

Resource-driven degradation is scoped to *functionality*, never to *security policy* — a
constrained device may run a smaller reasoning model, but it never runs a weaker capability-token
check, a shallower Trust Boundary than its `TenancyMode` requires, or a skipped consent step; the
`fallback_order` in Algorithms §1 has no substitution that touches
[15 — Security Architecture](15-security-architecture.md) policy, by construction. Multi-tenant
isolation is enforced as a hard boundary, not a logical convention: `TenancyMode::MultiTenantOrg`
mechanically raises minimum Trust Boundary depth (Algorithms §3), so tenant isolation survives even
a buggy or compromised Capability. Federation join requires mutual capability attestation, not mere
network reachability — a device on the same LAN or VPN is never auto-federated. Org fleet
revocation must cascade at the same bound as any other revocation
([03 §Algorithms](03-kernel-architecture.md#algorithms)) so an offboarded employee's device loses
every delegated authority immediately, not on its next check-in. Full threat enumeration for
federation and multi-tenancy specifically is in [17 — Threat Model](17-threat-model.md).

## Failure Modes

- **Degradation lag.** A hardware constraint (thermal, memory pressure) worsens faster than the
  admission-time degradation policy reacts, risking an OOM or thermal shutdown before a cheaper
  tier is applied.
- **KG shard hot-spotting.** A single very active tenant, team, or viral shared Semantic Object
  concentrates load on one shard, degrading latency for that partition only.
- **Federation split-brain.** A network partition leaves two fleet members each believing they hold
  sync authority for shared state.
- **Cross-tenant boundary misconfiguration.** An incorrectly scoped `CapabilityGatedEdge` could, in
  principle, leak read access across a tenant boundary if not caught by testing (§Testing Strategy).
- **Explanation lag.** A substitution is applied before its `user_notice` is surfaced, momentarily
  producing lower-fidelity results with no visible explanation — indistinguishable, from the user's
  side, from silent degradation, which [02 §4](02-core-architecture.md#4-design-invariants)
  explicitly forbids.
- **Autoscale lag at enterprise scale.** A sudden fleet-wide load spike outpaces horizontal
  Agent Runtime pod autoscaling, producing queueing delay.

## Recovery Mechanisms

A watchdog tied to the same thermal/battery governor that scales `ResourceLedger.capacity`
([04 §Algorithms 3](04-scheduler.md#algorithms)) triggers degradation proactively at a
threshold *below* actual failure, rather than reactively after a fault — closing the degradation-lag
failure mode. Hot-spotted Knowledge Graph shards are split or replicated by a rebalancing job
defined in [29 — Database Schema](29-database-schema.md), triggered on sustained hot-shard
detection rather than manually. Federation split-brain is resolved on network heal by the
conflict-resolution mechanism [21 — Distributed Execution](21-distributed-execution.md) defines for
personal-mesh reconciliation, with an org-designated tie-break authority added for `OrgFleet` scope.
Cross-tenant boundary defects are treated as Sev-1 findings routed through
[15 — Security Architecture](15-security-architecture.md)'s incident process the moment continuous
fuzzing (§Testing Strategy) surfaces one. Explanation lag is closed structurally, not procedurally:
`apply_and_explain` in the Pseudocode above writes the audit-log notice atomically with installing
the substituted Capability, so a substitution cannot exist without a corresponding explanation
already recorded. Autoscale lag is absorbed by the same `BatchDistributable` queuing and aging
mechanism [04 — Scheduler](04-scheduler.md#recovery-mechanisms) already defines, applied across
cluster nodes instead of cores.

## Performance Analysis

Minimum-viable hardware profile and growth path — every row runs the identical codebase; only
`HardwareProfile` values and the resolved Capability/model-tier mix change:

| Tier | Example device | RAM | Compute | Storage | Resident model tier | Concurrent Agents | Boot / workspace-gen |
|---|---|---|---|---|---|---|---|
| 1 — SBC | Raspberry Pi 5-class | 4–8 GB | ~4 TOPS NPU, 4-core ARM | 64–256 GB eMMC/USB-NVMe | Tiny/Edge only (0.3–1B) | 1, serialized | Meets [36](36-performance-benchmarks.md)'s targets with minimal margin; simpler Workspaces (fewer live Capabilities) |
| 2 — Laptop | Consumer ultrabook | 16–32 GB | 20–40 TOPS NPU + iGPU | 512 GB–2 TB NVMe | Small resident + Large on-demand | 2–4 concurrent | Meets [36](36-performance-benchmarks.md)'s default budget with full margin |
| 3 — Workstation | Dev/creator workstation | 32–128 GB | dGPU, 12–48 GB VRAM | Multi-TB NVMe | Large resident + Vision/Speech resident | 8+ concurrent | Meets targets comfortably; richer Workspaces; can serve as a `PersonalMesh` federation hub, offloading for other owned devices |
| 4 — Enterprise cluster | Multi-node GPU cluster | TBs, aggregate | Multi-GPU nodes, autoscaled | Shared/distributed storage | Large + specialized per tenant | 100s–1,000s across the fleet | Per-node boot/workspace-gen unchanged; fleet elastic scale-out is a separate SLA layered on top |

What changes at each boundary is capacity and the `tenancy` dimension, never the layer stack: SBC
to laptop unlocks the `Large` model tier and multi-Agent concurrency once NPU/VRAM headroom exists;
laptop to workstation unlocks resident heavy Vision/generation Capabilities and promotes the device
to a federation hub for a user's other devices; workstation to enterprise turns on
`TenancyMode::MultiTenantOrg` — Knowledge Graph partitioning, multi-tenant capability security,
autoscaled Agent Runtime pods, and `OrgFleet`-scope federation — on the same node design, not a
different one.

## Trade-offs

A single hardware-agnostic `CapacityDescriptor` abstraction ([03](03-kernel-architecture.md)) costs
some peak efficiency at both extremes — an SBC leaves device-specific micro-optimizations unused,
and an enterprise cluster leaves some cluster-specific scheduler tricks on the table — in exchange
for one codebase, one security model, and one test suite across the entire hardware range, directly
serving [02 §4](02-core-architecture.md#4-design-invariants) and
[01 §10](01-vision-and-philosophy.md#10-success-criteria). Substitution-based degradation preserves
usability ([01 §5](01-vision-and-philosophy.md#5-universal-usability-highest-priority)) far better
than hard feature-gating, at the cost of a wider quality-variance surface the user must be told
about — Hyperion accepts this because [18 — Explainability & Trust](18-explainability-and-trust.md)
makes the variance legible rather than hidden. Partitioning the Knowledge Graph per tenant is simple
and secure but can create a hot single shard for a very active household or team; finer
sub-partitioning reduces hot-spotting at the cost of rebalancing complexity — Hyperion starts coarse
and splits reactively (§Recovery Mechanisms) rather than pre-partitioning speculatively. Reusing the
personal-mesh federation protocol for organizational fleets is simpler to build and audit than a
second protocol, but couples org policy expressiveness to what a personal capability grant can
express; Hyperion resolves this by making org policy strictly attenuating, never additive, which is
a constraint, not a limitation the org can wish away.

## Testing Strategy

The hardware conformance matrix from [03](03-kernel-architecture.md#testing-strategy) and
[04](04-scheduler.md#testing-strategy) — spanning Raspberry Pi-class SBCs through multi-GPU
enterprise nodes — is the same matrix this document's benchmark suite runs against, extended with
the `tenancy` dimension. Golden degradation tests fixture a `HardwareProfile` and assert the exact
`DegradationPlan` produced — which Capability, which substitution, and whether the rendered
explanation matches the template — so a regression in fallback ordering is caught mechanically, not
by inspection. A fleet simulation harness spins up many virtual tenants and devices to fuzz
Knowledge Graph partitioning and the cross-tenant security boundary, specifically attempting reads
across `TenantId` without a `CapabilityGatedEdge` grant, which must always be denied. Federation
chaos tests — network partition, revocation race, split-brain reconciliation — mirror the
fault-injection philosophy of [03](03-kernel-architecture.md#testing-strategy) and
[04](04-scheduler.md#testing-strategy). Physical Raspberry Pi-class hardware, not only emulation, is
kept in continuous integration, because thermal and storage-latency characteristics at that tier are
difficult to emulate faithfully and are exactly where degradation logic is most safety-critical.

---
*Next: [38 — Five-Year Evolution](38-five-year-evolution.md).*
