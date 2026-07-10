# Implementation Phases

This document sequences the 40 subsystem specifications in this repository into ten
production-ready delivery phases. It is the operational counterpart to
[38 — Five-Year Evolution](38-five-year-evolution.md), which maps these same ten phases onto a
five-year horizon (Phases 1-4 = Year 1, Phases 5-7 = Year 2, Phases 8-10 = Year 3, with Years 4-5
being post-GA scale-out). Where that document answers "when do we reach GA and what comes after,"
this document answers "in what order do we build the 40 subsystems, and how do we know each phase
is actually done."

## 1. Sequencing Principle

Hyperion cannot be built outside-in (UI first, intelligence later) or top-down (business logic
first, kernel later) without contradicting its own Golden Rule
(see [01 — Vision & Philosophy](01-vision-and-philosophy.md)): a Workspace that isn't backed by a
real Intent Engine and real Capability routing is a mockup, not Hyperion. The phase order below
follows the layer stack in [02 — Core Architecture](02-core-architecture.md) §1 bottom-up (L0 → L6),
with two deliberate exceptions:

- **Security, privacy, explainability, and recovery (Phase 8) are pulled forward from L-cross-cutting
  status into their own hardening phase** rather than left until the end, because every prior
  phase already depends on primitives (capability tokens, recovery points) that
  [03 — Kernel Architecture](03-kernel-architecture.md) and
  [33 — Rollback & Recovery](33-rollback-recovery.md) define early — Phase 8 is where those
  primitives are *hardened and audited*, not where they are *invented*. A minimal version of
  capability security ships in Phase 1; Phase 8 is where [17 — Threat Model](17-threat-model.md)'s
  full attack catalog gets systematically tested against.
- **The Developer SDK and Plugin ecosystem (Phase 9) intentionally come after the runtime is
  functionally complete**, not before, because [25 — SDK](25-sdk.md) and
  [24 — Plugin Framework](24-plugin-framework.md) expose a stable contract surface — shipping them
  early would freeze APIs that Phases 1-8 still need to change.

## 2. Phase Overview

| Phase | Theme | Primary Documents | Year (per 38) |
|---|---|---|---|
| 1 | Core kernel, HAL, scheduler, IPC, capability security | [03](03-kernel-architecture.md), [04](04-scheduler.md), [30](30-ipc-framework.md), [31](31-event-system.md) | 1 |
| 2 | Semantic object store, knowledge graph, context engine | [28](28-storage-engine.md), [29](29-database-schema.md), [09](09-knowledge-graph.md), [06](06-context-engine.md), [07](07-context-propagation.md) | 1 |
| 3 | Local AI runtime, intent engine, planner, memory engine | [22](22-local-ai-runtime.md), [05](05-intent-engine.md), [08](08-memory-engine.md), [23](23-multi-model-orchestration.md) | 1 |
| 4 | Agent runtime, multi-agent orchestration, workflow execution | [11](11-agent-runtime.md), [12](12-multi-agent-coordination.md) | 1 |
| 5 | Dynamic UI generation, adaptive interface engine, accessibility | [13](13-dynamic-ui-runtime.md), [14](14-accessibility.md) | 2 |
| 6 | Semantic filesystem, workspace generation, universal search | [10](10-semantic-filesystem.md) (workspace generation builds on 13; universal search builds on 09) | 2 |
| 7 | Distributed execution, device federation, networking | [21](21-distributed-execution.md), [20](20-device-framework.md), [19](19-networking-stack.md) | 2 |
| 8 | Privacy, security, rollback, recovery, explainability, observability | [16](16-privacy-architecture.md), [15](15-security-architecture.md), [17](17-threat-model.md), [33](33-rollback-recovery.md), [18](18-explainability-and-trust.md), [34](34-observability-telemetry.md) | 3 |
| 9 | Developer SDK, plugin ecosystem, capability marketplace | [25](25-sdk.md), [24](24-plugin-framework.md), [26](26-apis.md), [27](27-compatibility-layer.md) | 3 |
| 10 | Optimization, benchmarking, documentation, testing, release candidate | [35](35-testing-strategy.md), [36](36-performance-benchmarks.md), [37](37-scalability-roadmap.md), [32](32-update-system.md) | 3 |

[39 — Commercial Strategy](39-commercial-strategy.md) and
[40 — Open-Source Governance](40-open-source-governance.md) are not engineering phases; they run
in parallel starting in Phase 1 (a project cannot retrofit governance after the fact) and are
called out in §5.

## 3. Phase Definitions

### Phase 1 — Core Kernel, HAL, Scheduler, IPC, Capability Security
**Delivers:** the hybrid microkernel of [03](03-kernel-architecture.md) (HAL, driver model,
capability monitor, sandboxing spectrum), the unified multi-resource scheduler of
[04](04-scheduler.md), the capability-scoped IPC framework of [30](30-ipc-framework.md), and the
pub/sub event backbone of [31](31-event-system.md).
**Entry criteria:** target hardware reference platforms selected across the tiers in
[37 — Scalability Roadmap](37-scalability-roadmap.md) (an SBC and a workstation-class box, at
minimum).
**Exit criteria (Definition of Done):** the kernel boots to a shell on both reference platforms;
a capability token can be minted, delegated, attenuated, and revoked end-to-end across two
sandboxed processes; the scheduler admits and fairly shares CPU/GPU/RAM/battery load across
synthetic Real-Time-UI, Interactive, and Background classes per [04](04-scheduler.md)'s algorithm;
cold boot is measured (not yet optimized) against [36 — Performance Benchmarks](36-performance-benchmarks.md)'s
budget. No AI, no Knowledge Graph, no user-facing UI exists yet — this phase is invisible to an
end user by design.
**Key risk:** underestimating IPC overhead in the pure-capability model; mitigated by the
zero-copy and batching mechanisms [03](03-kernel-architecture.md) specifies up front rather than
retrofitting later.

### Phase 2 — Semantic Object Store, Knowledge Graph, Context Engine
**Delivers:** the write-ahead-log-backed storage engine of [28](28-storage-engine.md), the
concrete schema of [29](29-database-schema.md), the graph/embedding model of
[09 — Knowledge Graph](09-knowledge-graph.md), the context-assembly logic of
[06 — Context Engine](06-context-engine.md), and the propagation wire format of
[07 — Context Propagation](07-context-propagation.md).
**Entry criteria:** Phase 1's IPC and capability security are stable enough that storage-engine
services can run as unprivileged, capability-secured processes per [03](03-kernel-architecture.md).
**Exit criteria:** a Semantic Object can be created, related to another object via a typed edge,
embedded, versioned, and queried both by graph traversal and by vector similarity; a Context
Bundle can be assembled for a synthetic Intent and correctly bounded in size per
[06](06-context-engine.md)'s relevance-ranking algorithm. Still no Intent Engine, Agents, or UI —
this phase is exercised through direct API calls per [26 — APIs](26-apis.md)'s Knowledge Graph API
surface (implemented early/minimally here, hardened in Phase 9).
**Key risk:** schema decisions made here are expensive to reverse once Phase 6's Semantic
Filesystem and Phase 4's Agents depend on them; mitigated by [29](29-database-schema.md)'s
explicit versioning columns designed for future migration.

### Phase 3 — Local AI Runtime, Intent Engine, Planner, Memory Engine
**Delivers:** on-device model execution per [22 — Local AI Runtime](22-local-ai-runtime.md), the
routing scaffold of [23 — Multi-Model Orchestration](23-multi-model-orchestration.md) (single-model
routing only at this stage — full ensemble/fallback logic matures in Phase 9), Intent parsing and
the Intent Graph of [05 — Intent Engine](05-intent-engine.md), and the five memory tiers of
[08 — Memory Engine](08-memory-engine.md).
**Entry criteria:** Phase 2's Knowledge Graph and Context Engine are queryable, since Intent
grounding and memory retrieval both depend on them.
**Exit criteria:** a natural-language utterance produces a structured, decomposed Intent Graph
with sub-intent dependencies; a resident small model executes locally within
[36](36-performance-benchmarks.md)'s latency budget; the memory decay algorithm in
[08](08-memory-engine.md) runs against synthetic usage and produces sane recency/frequency/importance
scoring. This is the first phase where a developer can type a goal and see Hyperion produce a real
plan — still with no execution (no Agents yet) and no visible UI (a debug console suffices).
**Key risk:** local model capability trajectory (flagged in [38](38-five-year-evolution.md)) — if
on-device models can't yet meet the quality bar for reliable Intent decomposition, Phase 3 slips;
mitigated by keeping [23](23-multi-model-orchestration.md)'s cloud-fallback path (fully consent-gated
per [16 — Privacy Architecture](16-privacy-architecture.md)) available from the start rather than
architected in later.

### Phase 4 — Agent Runtime, Multi-Agent Orchestration, Workflow Execution
**Delivers:** the sandboxed Agent process model of [11 — Agent Runtime](11-agent-runtime.md) and
the task-allocation/shared-plan/conflict-resolution machinery of
[12 — Multi-Agent Coordination](12-multi-agent-coordination.md).
**Entry criteria:** Phase 3's Intent Engine produces Intent Graphs; Phase 1's scheduler and
capability security can admit and sandbox a new process class (Agents).
**Exit criteria:** the "Launch my product" worked trace in [12](12-multi-agent-coordination.md) runs
end-to-end against stub Capabilities (real Capabilities arrive in Phase 9's plugin ecosystem, but
Phase 4 needs *some* invokable functionality to prove orchestration — a small fixed set of
first-party Capabilities, e.g. web research and document drafting, is built in-house for this
purpose and later migrated onto the Phase 9 Plugin Framework); a deliberately-failed Agent is
contained without corrupting the shared goal state. **Year 1 ends here** per
[38](38-five-year-evolution.md) — the intelligence substrate is complete, but the product is still
developer-only.
**Key risk:** shared-plan conflict resolution is the least precedented subsystem in the entire
spec; mitigated by [35 — Testing Strategy](35-testing-strategy.md)'s adversarial/chaos testing
being introduced here rather than deferred to Phase 10.

### Phase 5 — Dynamic UI Generation, Adaptive Interface Engine, Accessibility
**Delivers:** the compiler-pipeline UI generation of
[13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md) and the accessibility-tree-as-first-class-output
model of [14 — Accessibility](14-accessibility.md).
**Entry criteria:** Phase 4's Agents produce results that need somewhere to render; Phase 3's
Context Engine supplies the Adaptive Complexity signal.
**Exit criteria:** the "prepare for my exam" worked Workspace in
[13](13-dynamic-ui-runtime.md) renders from a real Intent Graph and real (if still limited)
Capability set, meets the sub-second generation budget in
[36](36-performance-benchmarks.md), and passes the automated accessibility linter in
[14](14-accessibility.md) with zero exceptions. This is the first phase an end user (not just a
developer) can meaningfully use Hyperion.
**Key risk:** accessibility retrofitted after visual design is the single most common failure mode
in UI platforms; mitigated by [14](14-accessibility.md)'s architectural requirement that the
accessibility tree is a byproduct of the same compiler pass as visual layout, enforced as a
Phase 5 exit gate, not a later cleanup pass.

### Phase 6 — Semantic Filesystem, Workspace Generation, Universal Search
**Delivers:** the query-as-navigation view layer of
[10 — Semantic Filesystem](10-semantic-filesystem.md) over the Phase 2 Knowledge Graph, the
POSIX-compatibility shim needed by Phase 9's compatibility layer, and full "reasoning becomes
search" query resolution (the "find the paper about quantum computing" example from
[09](09-knowledge-graph.md), now exposed through both natural-language Intent and the semantic
filesystem view).
**Entry criteria:** Phase 5's Workspace generation exists so search results have somewhere to be
displayed; Phase 2's Knowledge Graph has enough real object volume (from Phases 3-5's dogfooding)
to make search meaningful to test.
**Exit criteria:** "show everything related to my vacation" (or an equivalent dogfooded query)
assembles a correct cross-type result set; a legacy path-based tool can read/write through the
POSIX shim without corrupting the underlying Semantic Objects.
**Key risk:** the shim's write path is the highest-risk data-integrity surface in the whole
system; mitigated by requiring [33 — Rollback & Recovery](33-rollback-recovery.md)'s recovery-point
mechanism (built in Phase 1/8, exercised here) around every shim write from day one.

### Phase 7 — Distributed Execution, Device Federation, Networking
**Delivers:** the device-trust-federation and work-placement model of
[21 — Distributed Execution](21-distributed-execution.md), the Device Object model of
[20 — Device Framework](20-device-framework.md), and the semantic web layer of
[19 — Networking Stack](19-networking-stack.md).
**Entry criteria:** a single-device Hyperion instance (Phases 1-6) is stable enough to be the unit
that gets federated.
**Exit criteria:** a Context Bundle and in-flight Agent checkpoint migrate from one physical device
to another mid-task ("continue on my phone"); a second, lower-tier device (per
[37](37-scalability-roadmap.md)'s hardware table) joins a federation and receives offloaded work
from [04](04-scheduler.md)'s scheduler without a privacy-tier violation. **Year 2 ends here** per
[38](38-five-year-evolution.md) — Hyperion is now a usable, if unhardened, daily-driver OS.
**Key risk:** consistency under partition (a phone going offline mid-sync); mitigated by
[28](28-storage-engine.md)'s CRDT/Merkle-diff sync design being validated here under induced
network partitions, not assumed correct from the spec alone.

### Phase 8 — Privacy, Security, Rollback, Recovery, Explainability, Observability
**Delivers:** the full privacy-tier enforcement of
[16 — Privacy Architecture](16-privacy-architecture.md), the hardened cross-layer capability
security and risk-assessment engine of
[15 — Security Architecture](15-security-architecture.md), systematic red-teaming against every
attack surface in [17 — Threat Model](17-threat-model.md), the recovery-point and undo machinery
of [33 — Rollback & Recovery](33-rollback-recovery.md), the Explanation Record system of
[18 — Explainability & Trust](18-explainability-and-trust.md), and the audit-grade observability
pipeline of [34 — Observability & Telemetry](34-observability-telemetry.md).
**Entry criteria:** Phases 1-7 exist end-to-end, so there is a real system to audit rather than a
paper design — this phase is explicitly a hardening pass, not greenfield construction (minimal
versions of all six subsystems above already exist from earlier phases; Phase 8 is where they
reach production rigor).
**Exit criteria:** every attacker-goal/mitigation pair in [17](17-threat-model.md) has a passing
regression test per [35 — Testing Strategy](35-testing-strategy.md); every autonomous action
across Phases 3-7 produces a queryable Explanation Record; a risky action (deleting many Semantic
Objects) correctly triggers backup-then-confirm per [15](15-security-architecture.md)'s risk
engine rather than a blanket dialog; a corrupted mid-Agent-execution crash recovers cleanly via
[33](33-rollback-recovery.md).
**Key risk:** hardening work is where schedule pressure most often causes silent scope-cutting;
mitigated by making every item in this phase's exit criteria a named, testable gate rather than a
qualitative "feels secure" judgment.

### Phase 9 — Developer SDK, Plugin Ecosystem, Capability Marketplace
**Delivers:** the Capability SDK and testing harness of [25 — SDK](25-sdk.md), the
plugin manifest/sandboxing/registry system of [24 — Plugin Framework](24-plugin-framework.md), the
hardened system-facing API gateway of [26 — APIs](26-apis.md), and legacy application support via
[27 — Compatibility Layer](27-compatibility-layer.md). The first-party Capabilities stubbed in
Phase 4 are migrated onto this real Plugin Framework.
**Entry criteria:** Phase 8's security hardening is complete — a Plugin ecosystem opened before
the capability-security model is hardened is a threat-model violation, not an MVP shortcut.
**Exit criteria:** a third-party developer (not the core team) builds, tests locally, and
publishes a Capability using only [25](25-sdk.md)'s public tooling, and the Model Router in
[23 — Multi-Model Orchestration](23-multi-model-orchestration.md) correctly selects between it and
a first-party equivalent; a legacy Windows/Linux/Android application runs inside
[27](27-compatibility-layer.md)'s Trust Boundary without corrupting the Knowledge Graph. **Year 3
begins transitioning to GA here** per [38](38-five-year-evolution.md).
**Key risk:** a rushed-open ecosystem invites the exact supply-chain attack surface
[17](17-threat-model.md) catalogs; mitigated by the manifest review gate in
[24](24-plugin-framework.md) being a hard Phase 9 exit-criterion, not a post-launch addition.

### Phase 10 — Optimization, Benchmarking, Documentation, Extensive Testing, Release Candidate
**Delivers:** the full benchmark suite and regression-gating pipeline of
[36 — Performance Benchmarks](36-performance-benchmarks.md), the scale-down/scale-up validation of
[37 — Scalability Roadmap](37-scalability-roadmap.md) across the full hardware tier table, the
complete layered test suite of [35 — Testing Strategy](35-testing-strategy.md) (unit through
adversarial), and the staged-update infrastructure of [32 — Update System](32-update-system.md)
needed to ship a GA release safely and to patch it afterward.
**Entry criteria:** Phases 1-9 are feature-complete; this phase adds no new subsystem, only
hardens and validates existing ones — a rule enforced deliberately so GA is not a moving target.
**Exit criteria (GA gate):** all numeric targets in [36](36-performance-benchmarks.md) are met on
every hardware tier in [37](37-scalability-roadmap.md), from Raspberry-Pi-class through
enterprise; the full [35](35-testing-strategy.md) suite passes, including the threat-model
regression suite from Phase 8; a staged update through [32](32-update-system.md) can be applied
and rolled back on a live system without data loss. This is the Release Candidate referenced by
[38 — Five-Year Evolution](38-five-year-evolution.md)'s Year 3 GA milestone — deliberately
criteria-gated, not calendar-gated.
**Key risk:** performance regressions reintroduced by last-minute Phase 9 ecosystem changes;
mitigated by [36](36-performance-benchmarks.md)'s regression-gating pipeline running continuously
from Phase 1 onward, so Phase 10 is a final full-matrix validation, not the first time these
benchmarks are run.

## 4. Cross-Phase Constants

Two subsystems are not tied to a single phase because they are load-bearing from Phase 1 onward:

- **[02 — Core Architecture](02-core-architecture.md)'s six design invariants** are checked at
  every phase exit, not just at Phase 8 or Phase 10.
- **[35 — Testing Strategy](35-testing-strategy.md)'s layered approach** applies from Phase 1: L0-L2
  deterministic testing starts in Phase 1, golden-path Intent regression starts in Phase 3, and
  adversarial/chaos testing starts in Phase 4 — Phase 10 is a comprehensive final pass across all
  of these, not their introduction.

## 5. Parallel, Non-Engineering Tracks

[39 — Commercial Strategy](39-commercial-strategy.md) and
[40 — Open-Source Governance](40-open-source-governance.md) run alongside all ten phases starting
at Phase 1: the governance model's core-vs-ecosystem boundary must be decided before Phase 9 opens
the Plugin ecosystem it governs, and the commercial strategy's hardware-tier partnerships depend on
the same reference platforms selected as a Phase 1 entry criterion.

---
*This is the final document in the specification. Return to [00 — Index](00-index.md) for the
complete table of contents, or [01 — Vision & Philosophy](01-vision-and-philosophy.md) to start
from the beginning.*
