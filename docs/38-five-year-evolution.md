# Five-Year Evolution

## Purpose

This document projects Hyperion's trajectory across five years, from the first bootable kernel to
a globally distributed, ecosystem-supported operating system. It answers a question none of the
subsystem documents can answer individually: **in what order do we build, for whom, and what has
to be true at each milestone for the next one to be reachable?** It sits above
[41 — Implementation Phases](41-implementation-phases.md), which defines the authoritative 10-phase
build plan in engineering detail (dependencies, exit criteria, staffing shape per phase); this
document groups those ten phases into three annual horizons plus a two-year post-GA horizon, and
attaches the business, market, and technology-dependency lens that a pure phase plan does not
carry. Where [41](41-implementation-phases.md) is the authority on *what* each phase delivers, this
document is the authority on *when a phase should ship relative to the market* and *what could
derail that schedule*.

## Motivation

An intent-native OS is unusually vulnerable to being built in the wrong order. It is technically
possible to build [13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md) before
[05 — Intent Engine](05-intent-engine.md) exists, or to chase
[21 — Distributed Execution](21-distributed-execution.md) before a single device is trustworthy —
but both would produce demos that cannot survive contact with the invariants in
[02 — Core Architecture §4](02-core-architecture.md#4-design-invariants). Sequencing therefore has
to satisfy three constraints simultaneously:

1. **Dependency order.** Higher layers in the [layered system view](02-core-architecture.md#1-layered-system-view)
   are meaningless without the layer beneath them; Phase order in
   [41 — Implementation Phases](41-implementation-phases.md) already encodes this and is not
   repeated here.
2. **Trust order.** Per [01 — Vision & Philosophy §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable),
   Hyperion earns the right to act autonomously; it should not be marketed to people who cannot yet
   verify that autonomy before the hardening work in Phase 8 lands.
3. **Market order.** A pre-1.0 intent-native OS is a developer tool, not a consumer product. Every
   year of this plan names the audience that year is actually built for, so that go-to-market
   sequencing (detailed in [39 — Commercial Strategy](39-commercial-strategy.md)) never gets ahead
   of what the engineering can honestly support.

## Strategy: The Five-Year Horizon

### Year 1 — Core OS and intelligence substrate (Phases 1–4)

| Phase (see [41](41-implementation-phases.md)) | Delivers | Layer |
|---|---|---|
| 1 — Foundation | Kernel, HAL, scheduler, IPC, capability security | L0–L1 |
| 2 — Knowledge substrate | Semantic object store, knowledge graph, context engine | L3, part of L4 |
| 3 — Cognition | Local AI runtime, intent engine, planner, memory engine | L4 |
| 4 — Coordination | Agent runtime, multi-agent orchestration, workflow execution | L4–L5 |

By the end of Year 1, Hyperion boots on developer reference hardware, understands a natural-
language intent, decomposes it, and executes it through coordinated agents against a capability-
secured kernel — but with no generated UI (a CLI/API surface stands in for
[13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md), which does not exist yet), no accessibility
layer, and single-device execution only. **Audience: kernel and platform engineers, evaluated on
whether the intelligence substrate is real, not on whether it is usable.** This is deliberately the
least demo-friendly year of the five; the temptation to front-load UI work for visibility is
rejected because Design Invariant 1 (no silent authority) and Invariant 5 (degrade, never fail
closed) must be load-bearing in the kernel and cognition layers before anything is built on top of
them.

### Year 2 — Usable daily-driver OS (Phases 5–7)

| Phase | Delivers | Layer |
|---|---|---|
| 5 — Experience | Dynamic UI generation, adaptive interface, accessibility | L6 |
| 6 — Knowledge surfaces | Semantic filesystem, workspace generation, universal search | L3/L6 |
| 7 — Reach | Distributed execution, device federation, networking | L1/L2 |

Year 2 is where Hyperion becomes something a person, not just an engineer, can live inside: intents
produce generated Workspaces (per [13](13-dynamic-ui-runtime.md)), files become Semantic Objects
addressable by meaning (per [10 — Semantic Filesystem](10-semantic-filesystem.md)), and
[14 — Accessibility](14-accessibility.md) is validated as a hard constraint on every generated
surface, not a checklist applied afterward. Cross-device continuity via
[21 — Distributed Execution](21-distributed-execution.md) turns Hyperion from a single-box OS into
a personal-fleet OS. **Audience: early adopters and enthusiasts running Hyperion as a genuine daily
driver on a second device**, per the developer/enthusiast-first sequencing in
[39 — Commercial Strategy](39-commercial-strategy.md).

### Year 3 — Hardened, ecosystem-ready, first GA (Phases 8–10)

| Phase | Delivers | Layer |
|---|---|---|
| 8 — Hardening | Privacy, security, rollback/recovery, explainability, observability | cross-cutting |
| 9 — Ecosystem | Developer SDK, plugin ecosystem, capability marketplace | L2 |
| 10 — Release | Optimization, benchmarking, documentation, testing, release candidate | cross-cutting |

Year 3 converts a working system into a trustworthy, extensible, shippable one:
[16 — Privacy Architecture](16-privacy-architecture.md) and
[15 — Security Architecture](15-security-architecture.md) close the gaps intentionally deferred in
Years 1–2, [33 — Rollback & Recovery](33-rollback-recovery.md) and
[18 — Explainability & Trust](18-explainability-and-trust.md) make Invariants 2 and 4 verifiable
rather than merely designed, and [24 — Plugin Framework](24-plugin-framework.md) plus
[25 — SDK](25-sdk.md) open Hyperion to third-party Capability developers ahead of any consumer
launch. **This is the first General Availability release**, targeted at the same enthusiast and
developer base as Year 2 but now with a supported upgrade path, a certification program (see
[40 — Open-Source Governance](40-open-source-governance.md)), and performance validated against the
targets in [36 — Performance Benchmarks](36-performance-benchmarks.md).

### Years 4–5 — Scale, partnerships, and expansion (post-GA)

With GA achieved, the roadmap shifts from "build the layers" to "extend the reach of the layers
already built":

- **Hardware partnerships and OEM preload.** Device-optimized Hyperion builds targeted at the
  hardware tiers defined in [37 — Scalability Roadmap](37-scalability-roadmap.md) — from
  Raspberry Pi-class embedded devices through flagship laptops — are packaged for OEM preload deals,
  the first channel through which Hyperion reaches non-technical consumers (Year 4 target).
- **Enterprise fleet features.** Multi-tenant management, policy-scoped Capability rollout, and
  fleet-wide observability, all specified in [37 — Scalability Roadmap](37-scalability-roadmap.md),
  reach production maturity and are commercially licensed per
  [39 — Commercial Strategy](39-commercial-strategy.md) (Year 4–5 target).
- **Capability Marketplace expansion.** The marketplace opened in Phase 9 grows from a curated set
  of first-party and reference Capabilities into a broad third-party ecosystem, governed by the
  certification program in [40 — Open-Source Governance](40-open-source-governance.md) and the
  manifest contract in [24 — Plugin Framework](24-plugin-framework.md).
- **International and accessibility-language expansion.** Locale-aware intent parsing, right-to-
  left and non-Latin script support in [13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md), and
  expanded assistive-technology coverage in [14 — Accessibility](14-accessibility.md) extend
  Hyperion beyond its initial English-first, accessibility-baseline release.

## Key Decisions & Rationale

- **Sequencing intelligence before interface (Year 1 before Year 2).** A generated UI over a weak
  Intent Engine would produce a system that looks like Hyperion but reasons like a chatbot with a
  UI generator bolted on — precisely the anti-pattern rejected in
  [01 — Vision & Philosophy §1](01-vision-and-philosophy.md#1-what-hyperion-is). Building the
  cognition layer first, even at the cost of a year with no consumer-visible product, keeps the
  golden rule in [01 §2](01-vision-and-philosophy.md#2-the-golden-rule) enforceable from day one.
- **Hardening before ecosystem, ecosystem before consumer scale.** Opening
  [24 — Plugin Framework](24-plugin-framework.md) to third parties before
  [15 — Security Architecture](15-security-architecture.md) and
  [16 — Privacy Architecture](16-privacy-architecture.md) are load-bearing would let a single bad
  Capability define Hyperion's reputation before the platform can contain the damage; Phase 8 is
  therefore ordered strictly before Phase 9 within the same year rather than run in parallel.
- **GA gated on hardening, not on feature completeness.** Year 3's GA criterion is "the six
  invariants in [02 §4](02-core-architecture.md#4-design-invariants) hold under adversarial testing
  and can be independently audited," not "every planned Capability exists." This keeps the
  temptation to ship a feature-complete-but-unverifiable system out of the critical path.

## Risks

| Horizon | Key risk | Mitigation |
|---|---|---|
| Year 1 | Local model capability trajectory undershoots what a real-time planner needs (see [22 — Local AI Runtime](22-local-ai-runtime.md)); consumer NPUs may not sustain interactive-latency decomposition. | [23 — Multi-Model Orchestration](23-multi-model-orchestration.md) keeps a cloud-fallback path available (opt-in only, per Invariant 3) so Year 1 milestones do not silently depend on a specific local-model breakthrough. |
| Year 2 | Dynamic UI generation quality (coherence, accessibility, aesthetic acceptability) lags user expectations set by static, hand-designed competitor UIs. | [14 — Accessibility](14-accessibility.md) conformance is a hard gate, not a target, catching regressions before they reach users; a library of vetted generation templates bounds worst-case output quality. |
| Year 3 | Hardening work (Phase 8) is inherently hard to schedule — security and privacy defects surface late. GA slips. | GA date is explicitly criteria-based, not calendar-based (see Key Decisions above); [41 — Implementation Phases](41-implementation-phases.md) exit criteria for Phase 8 are the actual gate. |
| Years 4–5 | OEM and enterprise partners request architecture-violating customizations (telemetry defaults, forced cloud routing) that would erode invariants for commercial reasons. | Addressed structurally in [40 — Open-Source Governance](40-open-source-governance.md), which places the six invariants outside any single commercial stakeholder's unilateral control. |

## Trade-offs

- **A slower, less demo-friendly Year 1 versus investor/market pressure to show a UI early.**
  Resolved in favor of substrate-first sequencing (see Key Decisions), accepting reduced early
  visibility as the cost of not compromising the golden rule.
- **Annual horizons versus the finer-grained phase boundaries in [41](41-implementation-phases.md).**
  This document intentionally under-specifies relative to 41 so the two documents do not drift out
  of sync; if this plan and 41's phase detail ever conflict, 41 is authoritative on engineering
  order and this document should be revised to match, not the reverse.
- **Committing to Years 4–5 activities (OEM, enterprise, marketplace scale) before Year 3 GA has
  actually shipped.** This is a forecasting risk accepted deliberately, because hardware and
  enterprise partnerships require 12–18 months of lead time that cannot start after GA without
  losing a full year of the five.

## Dependencies on Other Subsystems

This document is a synthesis layer and depends on nearly every subsystem document for its phase
content; the two documents it must never drift from are
[41 — Implementation Phases](41-implementation-phases.md) (engineering detail and exit criteria) and
[37 — Scalability Roadmap](37-scalability-roadmap.md) (the hardware tiers targeted in Years 4–5). It
supplies the timeline context consumed by [39 — Commercial Strategy](39-commercial-strategy.md)
(go-to-market sequencing) and [40 — Open-Source Governance](40-open-source-governance.md) (the
maturity milestones that gate governance transitions).

## Validation / Success Metrics

Each horizon has a falsifiable exit test rather than a date alone:

- **Year 1**: an unmodified natural-language intent, issued on reference developer hardware, is
  decomposed, scheduled, and executed end-to-end with a complete audit trail, with zero ambient-
  authority violations under adversarial [17 — Threat Model](17-threat-model.md) testing.
- **Year 2**: a panel of non-developer daily-driver testers completes a defined task set (per the
  worked examples in [05](05-intent-engine.md) and [13](13-dynamic-ui-runtime.md)) without falling
  back to a traditional OS, and 100% of generated Workspaces pass automated
  [14 — Accessibility](14-accessibility.md) conformance checks.
- **Year 3**: GA ships only once independent security and privacy audits confirm all six invariants
  in [02 §4](02-core-architecture.md#4-design-invariants) hold under the adversarial corpus in
  [17 — Threat Model](17-threat-model.md), and the performance targets in
  [36 — Performance Benchmarks](36-performance-benchmarks.md) are met on the lowest supported
  hardware tier.
- **Years 4–5**: signed OEM preload agreements, enterprise fleets in production per
  [37 — Scalability Roadmap](37-scalability-roadmap.md), and a Capability Marketplace whose
  third-party submission rate and certification pass rate (per
  [40 — Open-Source Governance](40-open-source-governance.md)) are tracked as leading indicators of
  ecosystem health rather than lagging revenue alone.

---
*Next: [39 — Commercial Strategy](39-commercial-strategy.md).*
