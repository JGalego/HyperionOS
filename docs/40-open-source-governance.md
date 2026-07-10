# Open-Source Governance

## Purpose

This document defines who gets to change Hyperion, how, and under what scrutiny — specifically,
which layers of the system in [02 — Core Architecture](02-core-architecture.md#1-layered-system-view)
are open source and governed as a shared commons, and which are commercially operated services
built on top of that commons per [39 — Commercial Strategy](39-commercial-strategy.md). It defines
the governance structure, contribution and review requirements, a compatibility/certification
program for third-party Capabilities and forks, and the mechanism that keeps the six invariants in
[02 — Core Architecture §4](02-core-architecture.md#4-design-invariants) from being silently
weakened by any single stakeholder, commercial or otherwise.

## Motivation

[01 — Vision & Philosophy §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable) and
[16 — Privacy Architecture](16-privacy-architecture.md) ask users to trust Hyperion with something
no prior OS asked for at this depth: continuous access to their goals, memory, and knowledge graph,
and the autonomous authority to act on their behalf. That trust cannot rest on a vendor's promise
alone — it has to rest on the ability of independent parties to verify the promise. Concretely:

- **"No silent authority crosses a Trust Boundary" (Invariant 1) is only checkable if the code that
  enforces Trust Boundaries is readable.** A closed-source capability-security kernel asks the user
  to trust the vendor's description of what the kernel does; an open one lets any sufficiently
  motivated party — a researcher, a competitor, a regulator, a single skilled user — check that the
  description is true. [15 — Security Architecture](15-security-architecture.md)'s guarantees are
  claims about code; only open code makes them falsifiable.
- **"Local-first by default" (Invariant 3) is a claim about what the system does *not* send over
  the network.** The only credible way to substantiate a negative claim like this at scale is
  publicly auditable networking and telemetry code (see
  [19 — Networking Stack](19-networking-stack.md) and
  [34 — Observability & Telemetry](34-observability-telemetry.md)).
- **A single commercial owner has a structural incentive, over a long enough horizon, to weaken
  exactly these invariants** — loosening local-first defaults drives premium cloud-tier revenue
  (see [39 — Commercial Strategy](39-commercial-strategy.md)); loosening capability security
  simplifies feature delivery. Open source alone does not prevent this if the vendor can still
  merge whatever it wants into the canonical repository; governance is the second, necessary
  control, layered on top of open licensing.

This document therefore treats "open source" and "governed independently of any single commercial
interest" as two separate requirements, both necessary, neither sufficient alone.

## Strategy: What Is Open, What Is Commercial

The boundary follows the layer boundary already established in
[02 — Core Architecture §1](02-core-architecture.md#1-layered-system-view), not a feature-by-feature
negotiation:

| Scope | License posture | Rationale |
|---|---|---|
| **L0 Kernel** — HAL, driver model, capability security, process/memory primitives ([03](03-kernel-architecture.md)) | Open source (permissive license) | This is the enforcement point for every trust claim in [15](15-security-architecture.md) and [16](16-privacy-architecture.md); it must be independently auditable to be believable. |
| **L1 System Runtime** — scheduler, IPC framework, sandboxing ([04](04-scheduler.md), [30](30-ipc-framework.md)) | Open source (permissive license) | Directly implements Invariants 1 and 3; same rationale as L0. |
| **L2 Platform Services** — capability registry, plugin framework, storage engine, event system, update system, networking stack ([24](24-plugin-framework.md), [28](28-storage-engine.md), [31](31-event-system.md), [32](32-update-system.md), [19](19-networking-stack.md)) | Open source (permissive license), reference implementation | The manifest and permission contracts here are the interop surface third parties build against; closing them would fragment the ecosystem the Marketplace in [39](39-commercial-strategy.md) depends on. |
| **L3–L6** — knowledge graph, cognition, coordination, experience layers | Open source core, with commercially operated *hosted services* layered on top (premium model routing, hosted Marketplace, fleet management console) | The reference implementations are open so the system is fully buildable and forkable end to end; the hosted conveniences around them are what [39 — Commercial Strategy](39-commercial-strategy.md) monetizes. |
| **Capability Marketplace hosting, billing, certification infrastructure** | Proprietary, commercially operated | This is a service, not an invariant-bearing subsystem; its absence does not prevent a fork from being a complete, trustworthy Hyperion. |
| **Premium cloud-inference tier operation** | Proprietary, commercially operated | Same reasoning; strictly additive per Design Invariant 5, never load-bearing for core functionality. |

The test applied to every new subsystem as it is designed: **if this code enforces or verifies one
of the six invariants in [02 §4](02-core-architecture.md#4-design-invariants), it is open by
default; if it is a convenience service built on top of already-open, already-verifiable
primitives, it may be commercially operated.**

## Governance Structure

Hyperion adopts a **staged governance model** rather than choosing permanently between a foundation
and a single-vendor model at launch, because neither extreme fits every stage of the five-year plan
in [38 — Five-Year Evolution](38-five-year-evolution.md):

- **Years 1–3 (pre-GA through GA): single-vendor-led with open contribution.** The originating
  organization funds and staffs the core (kernel, security, cognition layers) because no
  independent contributor base yet exists to sustain that pace — the same reasoning
  [39 — Commercial Strategy](39-commercial-strategy.md) uses to justify developer-first, pre-revenue
  sequencing. Code and RFCs are public from day one; external contributions are accepted under the
  process below; but the originating organization holds the deciding vote on the Technical Steering
  Committee (TSC) during this period, disclosed openly rather than left implicit.
- **A published transition trigger, not an indefinite single-vendor model.** Once Hyperion reaches
  a defined maturity milestone — GA plus a minimum count of independent (non-employee) TSC-eligible
  contributors sustained over two consecutive release cycles — governance transitions to an
  **independent foundation**, modeled on precedents like Kubernetes' donation to the CNCF: the
  trademark, the RFC process, and TSC seat allocation move to a nonprofit steward with a board that
  is majority non-originating-organization. This trigger is written into the project's governance
  charter at launch, not left as a future promise, so it cannot be quietly deferred.

**Technical Steering Committee (TSC).** Seven seats:

```
TSC (7 seats)
 ├─ 3 seats — originating organization (Years 1–3), reducing to 2 post-foundation-transition
 ├─ 2 seats — elected by active core contributors (any org, including competitors and forks)
 ├─ 1 seat  — reserved for security/privacy (nominated by the security working group, see below)
 └─ 1 seat  — reserved for accessibility (nominated by the accessibility working group)
```

The reserved accessibility seat is a deliberate governance expression of
[01 — Vision & Philosophy §5](01-vision-and-philosophy.md#5-universal-usability-highest-priority),
which names accessibility as a priority that outranks raw capability whenever the two are in
tension — a priority that would otherwise be the first thing traded away under commercial or
schedule pressure. The reserved security/privacy seat exists for the analogous reason relative to
[15](15-security-architecture.md) and [16](16-privacy-architecture.md).

**RFC process for core-invariant changes.** Ordinary changes (a new Capability type, a scheduler
heuristic, a UI generation template) follow a standard pull-request review. A change that touches
one of the six invariants in [02 — Core Architecture §4](02-core-architecture.md#4-design-invariants) —
for example, any change to what "local-first by default" or "no silent authority" means in
practice — requires:

1. A public RFC with an explicit "invariant impact" section.
2. A minimum 30-day public comment period.
3. An 80% supermajority TSC vote, not a simple majority — deliberately harder to reach than any
   other governance decision, so that no single TSC bloc (including the originating organization's
   seats) can unilaterally weaken an invariant.
4. A published rationale, permanently archived, regardless of outcome — including rejected
   proposals, so the historical record of "someone proposed weakening this, and why it was
   rejected" is itself part of the trust surface for
   [18 — Explainability & Trust](18-explainability-and-trust.md).

## Contribution and Code-Review Requirements

Trust bar scales with blast radius, mirroring the capability-security principle in
[02 — Core Architecture §5](02-core-architecture.md#5-capability-security-as-the-unifying-security-model)
that the sandbox — not code review alone — is the safety net for lower-trust code:

| Tier | Scope | Review bar |
|---|---|---|
| Tier 0 | L0 kernel, capability security ([03](03-kernel-architecture.md)), IPC ([30](30-ipc-framework.md)) | Two independent maintainer approvals plus mandatory security-working-group sign-off; critical capability-check paths require the test coverage and, where applicable, formal-verification treatment specified in [35 — Testing Strategy](35-testing-strategy.md). No single-approver merges, ever, regardless of seniority. |
| Tier 1 | L1–L2 platform services ([04](04-scheduler.md), [28](28-storage-engine.md), [31](31-event-system.md)) | One maintainer approval plus automated invariant-conformance tests (below) passing. |
| Tier 2 | L3–L6 reference implementations, first-party Capabilities | Standard single-maintainer review; the capability-security sandbox in [15](15-security-architecture.md) is the primary containment mechanism, so review focuses on correctness and quality, not exhaustive threat modeling per change. |
| Ecosystem | Third-party Capabilities distributed via the Marketplace | No code review by Hyperion at all by default — contained entirely by the manifest-declared permission model in [24 — Plugin Framework](24-plugin-framework.md) and the runtime sandbox; certification (below) is opt-in and orthogonal to distribution. |

This tiering is what makes an unusually high trust bar for kernel code compatible with a thriving
low-friction ecosystem: the two are not in tension once the sandbox, not review, is doing the
containment work at the ecosystem tier.

## Compatibility and Certification Program

Two related but distinct certifications, both anchored to the manifest format defined in
[24 — Plugin Framework](24-plugin-framework.md) as the interoperability contract:

- **"Hyperion Certified Capability"** — automated conformance testing that a Capability's manifest
  accurately declares its permissions and side effects (no undeclared capability escalation), that
  it degrades per Design Invariant 5 rather than failing closed, and that any generated UI it
  contributes passes [14 — Accessibility](14-accessibility.md) conformance. This is the badge shown
  in the Marketplace UI and is a prerequisite for the revenue-sharing status described in
  [39 — Commercial Strategy](39-commercial-strategy.md), but is independent of the Marketplace
  itself — any Hyperion installation, including a fork, can run the certification suite locally.
- **"Hyperion Compatible"** — a conformance test suite a fork or independent implementation runs
  against itself, verifying that all six invariants in
  [02 §4](02-core-architecture.md#4-design-invariants) hold and that the L2 manifest/permission
  contract in [24](24-plugin-framework.md) is honored byte-for-byte, so that a Capability certified
  against one Hyperion-Compatible implementation runs correctly on any other. Modeled on the
  Kubernetes Conformance Program and POSIX certification: the goal is that "Hyperion-compatible"
  is a testable claim, not a marketing one, and that the ecosystem can survive and interoperate
  even if the originating organization's own distribution is not the only implementation.

Both certifications are themselves open-source test suites, versioned alongside the core, and
subject to the same RFC process for any change that alters what "compatible" means.

## Key Decisions & Rationale

- **Staged governance (single-vendor-led, then foundation) rather than a permanent choice at
  launch** — chosen because a foundation with no founding contributor base cannot execute the
  Year 1 substrate-first plan in [38 — Five-Year Evolution](38-five-year-evolution.md), but an
  indefinite single-vendor model cannot honestly claim the independence the trust argument in
  Motivation requires. Writing the transition trigger into the charter at launch is the mechanism
  that makes the "staged" promise credible rather than indefinitely deferrable.
- **Supermajority, not majority, for invariant changes** — a simple majority would let the
  originating organization's guaranteed TSC seats alone approve an invariant change during Years
  1–3; 80% forces at least one non-originating-organization seat (elected contributor, security, or
  accessibility) to concur.
- **Sandbox-first containment for ecosystem code, review-first containment for kernel code** —
  reviewing every third-party Capability line-by-line does not scale and contradicts the entire
  premise of capability security in [15](15-security-architecture.md); the tiering above puts
  review effort exactly where the sandbox cannot substitute for it.

## Risks

- **The originating organization slow-walks the foundation-transition trigger** by disputing whether
  the maturity milestone has been met. Mitigated by defining the trigger in measurable terms
  (release count, independent-contributor count) in the charter itself, auditable by any TSC member.
- **A well-resourced fork forks the brand, not just the code**, confusing users about what
  "Hyperion" means. Mitigated by trademark control remaining with the foundation/steward regardless
  of code license, and by the "Hyperion Compatible" certification giving legitimate forks a
  positive, testable identity instead of the ambiguous default of "unofficial."
- **Tier 2/Ecosystem's low review bar becomes a security incident vector** despite sandboxing, if a
  sandbox escape is found. Mitigated by treating any confirmed sandbox escape as a Tier-0-severity
  incident regardless of where the vulnerable Capability sits, per
  [17 — Threat Model](17-threat-model.md).
- **RFC fatigue** — routing too much through the heavyweight invariant-change process slows
  legitimate evolution. Mitigated by keeping the invariant list in
  [02 §4](02-core-architecture.md#4-design-invariants) deliberately short (six items) so the
  heavyweight path is rare by design, not by discipline alone.

## Trade-offs

- **Openness of L0–L2 vs. commercial defensibility.** Publishing the kernel and capability-security
  model gives competitors a working reference implementation to build against; accepted because
  the trust this openness buys, per Motivation, is worth more to the platform's adoption than the
  defensibility it costs — this is the same trade-off [39 — Commercial Strategy](39-commercial-strategy.md)
  makes explicitly when it pushes all commercial differentiation into hosted services above the
  open core rather than into the core itself.
- **A reserved accessibility TSC seat vs. a fully merit-elected board.** This sacrifices some
  procedural purity (one seat is not competitively elected) to structurally guarantee that
  [01 §5](01-vision-and-philosophy.md#5-universal-usability-highest-priority)'s stated priority
  cannot be voted away by a TSC majority that does not weight it as heavily as the vision document
  does.
- **Sandbox-centric trust for ecosystem code vs. uniform high-bar review everywhere.** A uniform
  high bar would be safer per-Capability but would make the ecosystem too slow to reach the
  developer traction [39 — Commercial Strategy](39-commercial-strategy.md)'s Marketplace revenue
  depends on; the tiered model accepts marginally higher ecosystem-tier risk, contained by the
  sandbox rather than eliminated by review.

## Dependencies on Other Subsystems

The governance boundary is defined against the layer model in
[02 — Core Architecture](02-core-architecture.md#1-layered-system-view) and its six invariants
(§4). Certification depends on the manifest contract in
[24 — Plugin Framework](24-plugin-framework.md) and the accessibility conformance criteria in
[14 — Accessibility](14-accessibility.md). Tier-0 review requirements depend on the test and
verification tooling described in [35 — Testing Strategy](35-testing-strategy.md). The commercial
boundary this document enforces is the one proposed in
[39 — Commercial Strategy](39-commercial-strategy.md); the maturity milestones that trigger the
foundation transition are the Year 3+ milestones in
[38 — Five-Year Evolution](38-five-year-evolution.md).

## Validation / Success Metrics

- **Independent-contributor share of Tier 0/Tier 1 commits** tracked release over release as the
  leading indicator for the foundation-transition trigger — not a lagging metric checked only when
  a transition is proposed.
- **Zero invariant changes merged without a passed supermajority RFC vote** — enforced mechanically
  by requiring the RFC's TSC vote record as a merge precondition for any change touching the files
  or interfaces flagged as invariant-bearing.
- **"Hyperion Compatible" suite pass rate published per fork/distribution**, making compatibility a
  continuously visible, third-party-checkable fact rather than a one-time claim.
- **Time-to-foundation-transition tracked against the charter's published trigger conditions**,
  with any missed trigger publicly explained by the TSC rather than silently passed over.
- **A standing count of confirmed cases where a commercial proposal was rejected specifically for
  conflicting with an invariant**, published in the RFC archive — a healthy governance model should
  produce a nonzero, visible count of this over time, not zero (zero would suggest the pressure
  described in Motivation is not actually being tested).

---
*Next: [41 — Implementation Phases](41-implementation-phases.md).*
