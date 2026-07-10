# Commercial Strategy

## Purpose

This document specifies how Hyperion sustains itself commercially without contradicting the
architectural invariants that make it trustworthy. It defines four revenue mechanisms — hardware
partnerships, a Capability Marketplace revenue share, enterprise/fleet licensing, and an optional
premium cloud-inference tier — and the go-to-market sequence that pairs each mechanism to the
audience defined for that period in [38 — Five-Year Evolution](38-five-year-evolution.md). It is
the commercial counterpart to [40 — Open-Source Governance](40-open-source-governance.md), which
defines what stays open; this document defines what may be commercially differentiated, and draws
that line explicitly rather than leaving it to be discovered by users after the fact.

## Motivation

Most commercial strategies available to a personal-computing OS are foreclosed by
[01 — Vision & Philosophy](01-vision-and-philosophy.md) and
[16 — Privacy Architecture](16-privacy-architecture.md) before they can even be evaluated on
merit:

- **Selling user data is disqualified outright.** [16 — Privacy Architecture](16-privacy-architecture.md)
  treats the user's Semantic Objects, Context Bundles, and Memory as theirs, not Hyperion's, to
  monetize. A revenue model that requires monetizing that data would require weakening the privacy
  architecture to fund itself — a circular failure the strategy must not create.
- **Silent cloud upsell is disqualified outright.** Design Invariant 3,
  [02 — Core Architecture §4](02-core-architecture.md#4-design-invariants), requires local-first
  execution with cloud/remote as an explicit, consented upgrade. A business model that depends on
  quietly routing more computation to paid cloud infrastructure than the user asked for — the
  dominant pattern in "free" consumer software — is architecturally impossible to build honestly on
  top of Hyperion, and attempting it would be the single fastest way to destroy the trust the whole
  system depends on (see [18 — Explainability & Trust](18-explainability-and-trust.md)).
- **Attention-based advertising is disqualified in spirit**, even though no single document
  prohibits it by name: an OS whose UI is generated per-goal and torn down afterward
  ([13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md)) has no persistent surface to sell attention
  against, and inserting one would reintroduce exactly the "application competing for the user's
  time" pattern [01 §4](01-vision-and-philosophy.md#4-primary-design-philosophy) is designed to
  eliminate.

What remains is a genuinely narrower but more durable set of options: sell better hardware
experiences, take a fair share of value created by third-party developers, sell administrative
capability to organizations, and sell optional, consented, higher-capability compute — never sell
the user's data or attention. Each is detailed below.

## Strategy: Revenue Mechanisms

### 1. Hardware partnerships and OEM preload

Hyperion licenses device-optimized builds to OEMs, tuned to the hardware tiers defined in
[37 — Scalability Roadmap](37-scalability-roadmap.md) (embedded/Raspberry-Pi-class through
flagship laptop/workstation-class). The commercial unit is a per-device licensing fee plus, where
the OEM wants it, co-engineering services to tune [22 — Local AI Runtime](22-local-ai-runtime.md)
model selection to that device's NPU/GPU profile. This is the least invariant-sensitive revenue
line: it monetizes engineering effort and integration quality, not user behavior, and it directly
funds the hardware diversity the vision in [01](01-vision-and-philosophy.md) requires (Pi-class to
enterprise cluster) rather than working against it.

### 2. Capability Marketplace revenue share

Third-party developers build Capabilities against [25 — SDK](25-sdk.md), package them per the
manifest contract in [24 — Plugin Framework](24-plugin-framework.md), and list them on a hosted
Capability Marketplace. Hyperion takes a platform fee on paid Capabilities and paid Capability
subscriptions — structurally identical to an app-store revenue share, but scoped correctly for
Hyperion's unit of software: developers monetize a Capability's *contract* (what it does, at what
trust level), not an application's screen time. This is consistent with the open-core boundary in
[40 — Open-Source Governance §Compatibility & Certification](40-open-source-governance.md): the
Marketplace *hosting, discovery, billing, and certification service* is a commercial product; the
manifest format and Capability contract it is built on are open per
[24 — Plugin Framework](24-plugin-framework.md), so a developer is never locked into Hyperion's
hosted Marketplace to distribute a Capability, only choosing it for reach and trust.

### 3. Enterprise / fleet licensing

Organizations deploying Hyperion across a device fleet license the multi-tenant management,
policy-scoped Capability rollout, and fleet-wide observability features specified in
[37 — Scalability Roadmap](37-scalability-roadmap.md), priced per-seat or per-device. This is
conventional enterprise-software economics layered on top of an otherwise consumer-priced core OS,
analogous to how a database vendor prices operational tooling around a freely licensed engine — the
individual user's Hyperion is not made worse or more limited to make the enterprise tier viable.

### 4. Premium cloud-inference tier (strictly opt-in)

Users who want higher-capability reasoning than their device can run locally — a larger planning
model for [05 — Intent Engine](05-intent-engine.md) decomposition, a higher-fidelity model for a
demanding [11 — Agent Runtime](11-agent-runtime.md) task — may subscribe to Hyperion-operated cloud
inference. Three properties make this compatible with Invariant 3 rather than a violation of it:

- **Opt-in, not opt-out.** The tier is enabled per-Capability-invocation or per-session, never as a
  default, per [16 — Privacy Architecture](16-privacy-architecture.md)'s consent model.
- **Never required for core functionality.** Every Intent expressible in
  [05 — Intent Engine](05-intent-engine.md) must have a locally satisfiable execution path, even if
  slower or lower-fidelity, per Design Invariant 5 (degrade, never fail closed),
  [02 §4](02-core-architecture.md#4-design-invariants). The premium tier makes some tasks *faster or
  better*, never *possible for the first time*.
- **Disclosed, not silent.** Every cloud invocation is logged and explainable exactly like any other
  Capability call, per [18 — Explainability & Trust](18-explainability-and-trust.md) — a user can
  always ask "did that run locally or in the cloud, and why."

## Go-to-Market Sequencing

| Period (per [38](38-five-year-evolution.md)) | Primary customer | Commercial mechanism active | Rationale |
|---|---|---|---|
| Year 1 | Developers, platform engineers | None (pre-revenue; open-source core adoption only) | No product exists to sell yet; the priority is technical credibility, not monetization. |
| Year 2 | Enthusiasts, early adopters | Capability Marketplace opens (early, low-volume) | A daily-driver user base is the minimum needed for a marketplace to have both developers and buyers. |
| Year 3 | Enthusiasts + first OEM design wins | Marketplace at GA scale; first hardware partnership contracts signed | GA (Phase 10, per [41](41-implementation-phases.md)) is the credibility signal OEMs require before committing shelf space. |
| Year 4 | Consumers (via OEM preload), first enterprise pilots | OEM preload live; enterprise/fleet licensing launches; premium cloud tier launches | Consumer reach requires OEM distribution, which requires the Year 3 design wins to convert; enterprise requires the fleet features matured in [37](37-scalability-roadmap.md). |
| Year 5 | Global consumer, enterprise at scale | All four mechanisms operating concurrently; international expansion | Revenue diversification reduces dependence on any single mechanism, consistent with the Trade-offs below. |

Developer/enthusiast-first sequencing is deliberate: a Capability Marketplace and an enterprise
fleet product both require a credible, security-hardened, feature-real OS to sell into, which only
exists once [38 — Five-Year Evolution](38-five-year-evolution.md)'s Year 2–3 milestones are met.
Selling to consumers or enterprises earlier would require overstating Hyperion's actual maturity.

## Key Decisions & Rationale

- **Revenue from hardware and services before revenue from data or attention** — the reverse of the
  smartphone-era default. This is a direct consequence of [01](01-vision-and-philosophy.md) and
  [16](16-privacy-architecture.md) foreclosing the alternative, and it is treated here as a
  constraint to design around, not a temporary handicap to be quietly relaxed once Hyperion has
  market power.
- **The Marketplace takes a platform fee, not exclusivity.** Developers can self-host or
  side-distribute Capabilities that conform to the open manifest format in
  [24 — Plugin Framework](24-plugin-framework.md); the fee buys discovery, billing, trust
  certification, and reach, not the only path to market. This keeps the commercial layer
  competing on value, consistent with the open-core boundary defended in
  [40 — Open-Source Governance](40-open-source-governance.md).
- **The premium cloud tier is priced and marketed as a performance upgrade, never as unlocking
  functionality.** Product marketing copy claiming a feature is "cloud-only" would be a direct
  Invariant 3 violation and is treated as a shipped bug, not a pricing choice, if it ever occurs.

## Risks

- **Marketplace platform-fee pressure to tighten distribution exclusivity** as revenue expectations
  grow — mitigated structurally by keeping the manifest format open (per
  [24](24-plugin-framework.md)) so exclusivity could only be imposed by degrading the hosted
  service's convenience, not by technical lock-in, which is a reversible commercial choice rather
  than an architectural one.
- **Premium cloud-inference margin pressure to make local execution deliberately worse** (a
  classic freemium anti-pattern) — mitigated by making local-path quality a tested, published
  metric under [36 — Performance Benchmarks](36-performance-benchmarks.md) and
  [35 — Testing Strategy](35-testing-strategy.md), so regression would be externally visible.
- **OEM partners requesting telemetry or default-routing changes** that conflict with
  [16 — Privacy Architecture](16-privacy-architecture.md) as a condition of a preload deal — this is
  the same risk flagged in [38 — Five-Year Evolution §Risks](38-five-year-evolution.md) and is
  addressed at the governance level in
  [40 — Open-Source Governance](40-open-source-governance.md), not by case-by-case negotiation.
- **Enterprise buyers expecting on-prem control over policy that conflicts with per-user consent
  defaults** — resolved by scoping fleet policy to organization-owned devices and organization-
  consented data only, never overriding an individual's personal-device privacy settings.

## Trade-offs

- **Open core vs. commercial differentiation, addressed explicitly.** The kernel, capability
  security model, scheduler, and IPC framework (L0–L2, per
  [40 — Open-Source Governance](40-open-source-governance.md)) are open source specifically because
  the trust claims in [01](01-vision-and-philosophy.md), [15](15-security-architecture.md), and
  [16](16-privacy-architecture.md) are unverifiable if the enforcement code is closed — a user
  cannot trust "no silent authority crosses a Trust Boundary" from a vendor's word alone. Commercial
  differentiation is therefore pushed entirely into *services built on top of* the open core: hosted
  convenience (Marketplace, cloud inference), integration and support (OEM, enterprise), never into
  the trust-bearing layers themselves. The trade-off this accepts is a narrower monetizable surface
  than a fully proprietary competitor has — accepted deliberately, because the alternative sacrifices
  the trust the whole product depends on.
- **Fewer, more concentrated revenue mechanisms vs. many small ones.** Four clear mechanisms are
  easier to keep invariant-clean than a sprawl of monetization experiments; this trades some
  short-term revenue optionality for long-term architectural discipline.
- **Enterprise and OEM revenue arrive later (Year 3–4) than a conventional startup roadmap would
  prefer**, a direct consequence of the substrate-first sequencing decided in
  [38 — Five-Year Evolution](38-five-year-evolution.md); this strategy accepts a longer runway
  requirement in exchange for not compromising Year 1–2 architecture to accelerate revenue.

## Dependencies on Other Subsystems

Depends on [37 — Scalability Roadmap](37-scalability-roadmap.md) for the hardware tiers and fleet
features being licensed, [24 — Plugin Framework](24-plugin-framework.md) and
[25 — SDK](25-sdk.md) for the Marketplace's technical contract, [16 — Privacy Architecture](16-privacy-architecture.md)
for the consent model the premium tier must satisfy, and
[38 — Five-Year Evolution](38-five-year-evolution.md) for timing. Constrains and is constrained by
[40 — Open-Source Governance](40-open-source-governance.md), which this document treats as the
authority on exactly where the open/commercial boundary sits.

## Validation / Success Metrics

- **No mechanism ever requires weakening an invariant to hit a revenue target** — audited
  continuously by treating any product proposal that touches
  [02 §4](02-core-architecture.md#4-design-invariants) as an automatic escalation to the governance
  process in [40 — Open-Source Governance](40-open-source-governance.md), regardless of commercial
  upside.
- **Marketplace health measured by developer retention and third-party (non-first-party) revenue
  share over time**, not gross transaction volume alone — a Marketplace dominated by first-party
  Capabilities indicates the ecosystem strategy in [40](40-open-source-governance.md) is not
  working.
- **Premium cloud-tier attach rate tracked against local-path performance**, per
  [36 — Performance Benchmarks](36-performance-benchmarks.md): if attach rate rises because local
  performance degrades rather than because cloud capability genuinely exceeds it, that is treated as
  a regression to fix, not a revenue win.
- **Zero substantiated incidents, across the five-year plan, of a user being unable to complete a
  goal without paying** — tracked via the audit and complaint channel defined in
  [18 — Explainability & Trust](18-explainability-and-trust.md); any confirmed incident is a
  Design-Invariant-3 violation and is treated with the severity of a security defect.

---
*Next: [40 — Open-Source Governance](40-open-source-governance.md).*
