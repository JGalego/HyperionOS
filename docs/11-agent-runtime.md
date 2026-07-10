# Agent Runtime

This document defines the Agent Runtime, the [L4 Cognition Layer](02-core-architecture.md#1-layered-system-view)
subsystem responsible for instantiating, binding, scheduling, and retiring every Agent in
Hyperion. It is the concrete mechanism behind the abstract definition in
[02 — Core Architecture §2](02-core-architecture.md#2-shared-vocabulary): an Agent is "a
specialized reasoning process that consumes Intents and Context Bundles and invokes Capabilities
... scheduled, sandboxed, and audited exactly like any other computation; 'agent' is a role a
process plays, not a privileged primitive." Everything in this document exists to make that
sentence literally true at runtime, not merely aspirational. Team-level coordination between
multiple Agents working the same goal is defined in
[12 — Multi-Agent Coordination](12-multi-agent-coordination.md).

## 1. Purpose

The Agent Runtime turns a bound `(Intent, Context Bundle)` pair into a running, resource-governed,
Capability-secured process, and turns that process's outputs back into
[Semantic Object](09-knowledge-graph.md) writes and [31 — Event System](31-event-system.md)
notifications. It owns exactly four responsibilities:

1. **Instantiation** — spawning an Agent as an ordinary sandboxed computation, per the
   [03 — Kernel Architecture](03-kernel-architecture.md) capability-security model.
2. **Binding** — attaching the Intent and Context Bundle the Agent is meant to act on.
3. **Mediated invocation** — routing every Capability call an Agent makes through the
   [04 — Scheduler](04-scheduler.md) and the Capability Broker (§6.1), never letting an Agent call
   a Capability directly.
4. **Lifecycle governance** — progress reporting, checkpointing, quota enforcement, and
   termination.

It explicitly does **not** decide *what* an Agent should do (that is
[05 — Intent Engine](05-intent-engine.md) decomposition and [12](12-multi-agent-coordination.md)
allocation), and it does not decide how results are displayed (that is
[13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md)). The Agent Runtime is deliberately "boring"
infrastructure — a uniform substrate — precisely so that the interesting, probabilistic behavior
of any given Agent specialization can vary freely without ever touching the security or scheduling
model underneath it.

## 2. Motivation

The product brief calls for a wide roster of specialized Agents — Research, Coding, Writing,
Education, Finance, Design, Travel, Security, Automation, Planning, Scheduling, Healthcare
Information, Hardware Diagnostics, Developer — with more added over time through
[24 — Plugin Framework](24-plugin-framework.md). Two implementation paths were available for a
roster this size:

- **Special-case each Agent type** as a distinct kernel or platform primitive with bespoke
  permissions ("the Finance Agent may touch payment rails, the Security Agent may touch the
  credential store"). This is the path most assistant platforms take, and it fails
  [02's design invariant](02-core-architecture.md#4-design-invariants) of exactly one security
  model: every new Agent type would require reasoning about a new trust surface, and the count of
  special cases grows without bound as third-party Agents arrive via [24](24-plugin-framework.md).
- **Treat "Agent" as a role, not a primitive.** One runtime, one process model, one Capability
  grant mechanism, one scheduling and quota system. Specialization lives entirely in *which*
  Capabilities a given Agent's manifest declares and *which* Intents it is matched against — never
  in special kernel treatment.

Hyperion takes the second path. This is also what makes [01 — Vision & Philosophy §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable)
enforceable: if Agents could bypass normal scheduling and Capability mediation, "every autonomous
action is interruptible, undoable, auditable, observable, explainable, and modifiable" would be a
promise the platform could not actually keep for its most autonomous computations.

## 3. Architecture

### 3.1 Position in the Layered System

```
┌───────────────────────────── L5 Coordination Layer ──────────────────────────────┐
│        Multi-Agent Coordination (12) · Explainability & Audit Log (18)           │
└─────────────────────────────────────┬────────────────────────────────────────────┘
                                       │ spawn(manifest, intent, context_bundle)
┌───────────────────────────────────────▼──────────────────────────────────────────┐
│                         AGENT RUNTIME   (L4 Cognition Layer)                      │
│                                                                                    │
│  ┌────────────────┐   ┌───────────────────┐   ┌────────────────┐  ┌────────────┐ │
│  │ Agent Manifest  │──▶│  Agent Supervisor  │──▶│ Agent Instance  │─▶│ Checkpoint │ │
│  │ Registry (§4)   │   │ (spawn / watchdog) │   │ (state machine) │  │ / Teardown │ │
│  └────────────────┘   └───────────────────┘   └───────┬────────┘  └────────────┘ │
│                                                         │ invoke(capability, args) │
│                                              ┌──────────▼──────────┐              │
│                                              │  Capability Broker   │              │
│                                              │  (grant resolution)  │              │
│                                              └──────────┬──────────┘              │
└─────────────────────────────────────────────────────────┼─────────────────────────┘
        │ progress / lifecycle events           grant or deny                        │
┌───────▼────────────────┐               ┌─────────────────▼─────────────────────┐
│ Event System (31)       │               │ Capability Registry / Plugin          │
│                         │               │ Framework (24) · Security Arch. (15)  │
└─────────────────────────┘               └─────────────────┬─────────────────────┘
                                                              │ scheduled dispatch
                                                  ┌────────────▼────────────┐
                                                  │   Scheduler (04)         │
                                                  │   quotas · fair-share ·  │
                                                  │   rate limiting (§6.2)   │
                                                  └────────────┬────────────┘
                                                              │ sandboxed process/container
                                                  ┌────────────▼────────────┐
                                                  │ Kernel Capability         │
                                                  │ Security (03) — Trust     │
                                                  │ Boundary enforcement      │
                                                  └──────────────────────────┘
```

### 3.2 Process Model

An Agent is instantiated as **an ordinary sandboxed process** — a container or micro-VM, chosen
by [03 — Kernel Architecture](03-kernel-architecture.md) based on the trust tier of the Agent's
declared Capabilities, exactly as it would choose for any other computation. There is no kernel
object, syscall, or scheduling class named "agent." What exists is:

- an **Agent Manifest** (a declarative descriptor, §5.1) that the Kernel and Scheduler never read
  directly — only the Agent Runtime interprets it;
- a **process** whose capability grant set, resource cgroup/profile, and Trust Boundary were
  derived from that manifest at spawn time by the Agent Runtime, then handed to the Kernel as
  ordinary process-creation parameters.

This means the Kernel's and Scheduler's mental model never grows more complex as new Agent
specializations are added — from the Kernel's point of view, a Coding Agent and a spreadsheet
recalculation are the same kind of thing: a process with a capability set and a resource budget.

### 3.3 Lifecycle State Machine

```
 spawning ──▶ bound ──▶ executing ──▶ completed
    │           │           │  ▲  │
    │           │           │  │  ├──▶ waiting_on_capability ──▶ executing
    │           │           │  │  │        (blocked on a grant or scheduler slot)
    │           │           │  └──┘
    │           │           ├──────▶ suspended ──▶ checkpointed ──▶ executing (resume)
    │           │           │                                    └▶ terminated
    │           │           └──────▶ failed ──▶ terminated
    └───────────┴─────────────────────────────────────────────────▶ terminated (abort at any state)
```

- **spawning** — Agent Runtime resolves the manifest, requests process creation from
  [03](03-kernel-architecture.md), establishes the Trust Boundary.
- **bound** — the Intent and Context Bundle are attached (§5.2); nothing executes before binding.
- **executing** — the Agent reasons and issues Capability invocations, each arbitrated by the
  Scheduler (§6.1–6.2).
- **waiting_on_capability** — blocked on a grant decision (possible user consent prompt via
  [13](13-dynamic-ui-runtime.md)) or a Scheduler queue slot.
- **suspended / checkpointed** — the Agent's state is serialized (§5.3) so it can be resumed later
  or handed off, e.g. when [12](12-multi-agent-coordination.md) reallocates work, or the user
  pauses a long-running goal.
- **completed / failed / terminated** — terminal states; all three flow into the audit record
  (§5.4) and an [31 — Event System](31-event-system.md) notification.

## 4. Built-in Agent Specializations

Each built-in specialization is a manifest, not a new mechanism. The table gives each a one-line
contract: what it consumes, what it produces, and the typical Capabilities it declares.

| Agent | Input | Output | Typical Capabilities |
|---|---|---|---|
| Research | A question/topic Intent + Context Bundle | A synthesized findings [Semantic Object](09-knowledge-graph.md) with citations | `web.search`, `document.summarize`, `knowledge_graph.query` |
| Coding | A software task Intent + repository Context Bundle | Code changes / PR Semantic Object | `code.generate`, `code.test`, `vcs.commit`, `sandbox.execute` |
| Writing | A writing goal + audience/tone context | A drafted document Semantic Object | `text.generate`, `style.check`, `document.format` |
| Education | A learning goal + learner profile from [08 — Memory Engine](08-memory-engine.md) | Curriculum and practice Semantic Objects | `content.curate`, `quiz.generate`, `progress.track` |
| Finance | A financial goal/question + linked-account Context Bundle | Budget/analysis Semantic Object, proposed transactions | `ledger.read`, `forecast.compute`, `payment.initiate` (high-trust, consent-gated) |
| Design | A creative brief + brand assets | Visual asset Semantic Objects (mockups, logos) | `image.generate`, `layout.compose`, `asset.store` |
| Travel | A trip goal + calendar/location context | Itinerary Semantic Object, booking proposals | `search.flights`, `search.lodging`, `calendar.write`, `payment.initiate` |
| Security | A security Intent or a system-raised anomaly | Risk report, remediation actions | `threat_intel.query`, `credential.audit`, `policy.enforce` — itself runs under the stricter oversight of [15 — Security Architecture](15-security-architecture.md) |
| Automation | A detected or user-specified repetitive pattern | A reusable automation Semantic Object ("routine") | `event_system.subscribe`, `capability.compose`, `schedule.create` |
| Planning | A complex, multi-step goal | A decomposed Intent Graph / project-plan Semantic Object | `intent_engine.decompose`, `coordination.allocate` — the usual entry point into [12](12-multi-agent-coordination.md) team formation |
| Scheduling | A time-bound Intent + calendar Context Bundle | Calendar events, reminders | `calendar.read`, `calendar.write`, `notification.schedule` |
| Healthcare Information | A health-related informational question + consented health records | Plain-language explanation Semantic Object with citations and disclaimers | `medical_knowledge.query`, `document.summarize` — never granted `treatment.prescribe`-class Capabilities by policy |
| Hardware Diagnostics | A device malfunction Intent or telemetry anomaly | Diagnosis report, suggested or auto-applied remediation | `device_framework.probe` ([20](20-device-framework.md)), `telemetry.read` ([34](34-observability-telemetry.md)), `driver.reset` |
| Developer | A platform-level Intent (build a plugin, debug the OS) | SDK-scoped artifacts | `sdk.*` ([25](25-sdk.md)), `api.*` ([26](26-apis.md)), kernel diagnostics — the one built-in manifest with a broadened default grant set, still fully capability-secured, matched to the "developer" tier of [01 — Vision & Philosophy §6](01-vision-and-philosophy.md#6-adaptive-complexity) |

Third-party specializations registered through [24 — Plugin Framework](24-plugin-framework.md) use
the identical manifest schema (§5.1) — there is no built-in/third-party distinction at the runtime
level, only a distinction in which publisher signed the manifest and which trust tier that earns
it under [15 — Security Architecture](15-security-architecture.md).

## 5. Data Structures

### 5.1 Agent Manifest

```
enum TrustTier { System, Verified, Community }   // 1:1 with 15-security-architecture.md's
                                                   // provenance_tier 0/1/2; referenced by name
                                                   // (not re-declared) in 12's TaskNode

AgentManifest {
  manifest_id            : UUID
  specialization         : string            // e.g. "coding", "travel"
  publisher              : PublisherID        // Hyperion built-in, or a Plugin Framework publisher
  baseline_capabilities   : [CapabilityRef]    // granted at spawn without further prompt
  requestable_capabilities: [CapabilityRef]    // may be requested at runtime, subject to §6.1
  resource_profile        : ResourceProfile    // defaults per specialization, see 04-scheduler.md
  trust_tier              : TrustTier          // maps 1:1 onto 15's provenance_tier (0/1/2); a
                                                 // manifest that fails vetting entirely gets no
                                                 // trust_tier and cannot be spawned via this path
                                                 // at all (15 §4) — there is no fourth value
  sandbox_class           : enum { process, container, microvm }
  signature               : Signature          // verified per 15-security-architecture.md
}
```

### 5.2 Agent Instance & Binding

```
type AgentRunId = UUID   // alias of AgentInstance.instance_id; the identifier
                          // 33-rollback-recovery.md's UndoScope::AgentRun and
                          // Trigger::PreAgentRun carry — one Agent instance's execution is one run

AgentInstance {
  instance_id      : UUID
  manifest_id      : UUID
  state            : LifecycleState            // §3.3
  bound_intent     : IntentRef                  // node in an Intent Graph, see 05-intent-engine.md
  context_bundle   : ContextBundleRef           // see 07-context-propagation.md
  trust_boundary_id: UUID
  grants           : [CapabilityGrant]          // active tokens, §5.3
  quota_state      : QuotaState                 // §6.2
  parent_session   : CoordinationSessionRef?    // set only if spawned under 12-multi-agent-coordination.md
}
```

### 5.3 Capability Grant & Checkpoint

```
CapabilityGrant {
  token_id      : UUID
  capability_ref: CapabilityRef
  scope         : [SemanticObjectID]   // objects this grant may touch, never "all"
  expiry        : Timestamp
  revocation_hook: fn()
}

AgentCheckpoint {
  checkpoint_id : UUID
  instance_id   : UUID
  serialized_state: bytes        // reasoning trace, partial outputs, open grants
  plan_version  : uint64          // for team Agents, matches the Shared Plan version in 12
  created_at    : Timestamp
}
```

### 5.4 Agent Execution Record (audit)

Every state transition, Capability invocation, grant decision, and quota event is appended to an
**Agent Execution Record**, the per-instance audit trail consumed by
[18 — Explainability & Trust](18-explainability-and-trust.md) and
[34 — Observability & Telemetry](34-observability-telemetry.md). This record, not the Agent's own
self-report, is the authoritative answer to "why did this Agent do that?"

## 6. Algorithms

### 6.1 Capability Grant Resolution

When an Agent invokes a Capability, the Broker resolves a grant in this order: (1) is the
Capability in `baseline_capabilities` and already scoped to an object in the bound Context Bundle
— grant immediately, log it; (2) is it in `requestable_capabilities` — check the user's standing
consent policy in [15](15-security-architecture.md); if policy allows silent grant for this
scope, grant and log; if not, surface a consent prompt through
[13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md) and block the Agent in
`waiting_on_capability`; (3) not declared in the manifest at all — deny unconditionally, no
prompt, no exception; this is the enforcement point for [02's "no silent authority" invariant](02-core-architecture.md#4-design-invariants).

### 6.2 Resource Quota & Backpressure

Every Agent instance is a token-bucket consumer along four independent dimensions: CPU/GPU/NPU-ms
per scheduling window, model-token budget per window, Capability-calls per window, and a
wall-clock ceiling tied to its bound Intent. Buckets are refilled by the
[04 — Scheduler](04-scheduler.md) at a rate set by priority class:

1. **Interactive** — a user is actively watching this Agent (foreground Workspace).
2. **Coordinated** — part of an active team under [12](12-multi-agent-coordination.md).
3. **Background/autonomous** — unattended, lowest refill rate.

Across all instances belonging to one user session, the Scheduler additionally runs weighted
fair-share queuing so that no single Agent's exhausted bucket can be compensated by starving a
sibling's queue slot. A **circuit breaker** trips after *N* consecutive Capability failures within
one window, forcing the instance to `suspended` and emitting an escalation event — this is the
direct mechanism that stops one runaway Agent from consuming the resources every other Agent, and
the user's own interactive work, depends on.

### 6.3 Checkpoint / Resume

Checkpointing serializes reasoning state, open grants (revoked, not carried across — resume
re-requests them), and partial outputs, then tears down the process while preserving the
`AgentInstance` record in `checkpointed` state. Resume re-spawns a fresh sandboxed process,
re-binds the same Intent and Context Bundle (re-fetched, since Context may have changed —
see [07 — Context Propagation](07-context-propagation.md)), and rehydrates reasoning state before
returning to `executing`. Checkpoints are themselves recovery points under
[33 — Rollback & Recovery](33-rollback-recovery.md).

## 7. Interfaces / APIs

```
AgentRuntime.spawn(manifest_id, intent_ref, context_bundle_ref) -> instance_id
AgentRuntime.bind(instance_id, intent_ref, context_bundle_ref) -> ack
AgentRuntime.invoke(instance_id, capability_ref, args) -> CapabilityResult   // routed via §6.1/§6.2
AgentRuntime.checkpoint(instance_id) -> checkpoint_id
AgentRuntime.resume(checkpoint_id) -> instance_id
AgentRuntime.terminate(instance_id, reason) -> ack
AgentRuntime.subscribe(instance_id, event_types) -> EventStream             // see 31-event-system.md
AgentRuntime.describe(instance_id) -> AgentExecutionRecord                  // see 18-explainability-and-trust.md
```

All calls cross a Trust Boundary and are themselves Capability-secured — even the Agent Runtime's
own management API is not ambient authority; callers (typically [12](12-multi-agent-coordination.md)
or the [05 — Intent Engine](05-intent-engine.md)) must hold a `runtime.manage` grant.

## 8. Pseudocode

```python
def agent_supervisor_loop(instance: AgentInstance):
    instance.state = SPAWNING
    proc = kernel.spawn_sandboxed(
        sandbox_class=instance.manifest.sandbox_class,
        capability_grants=instance.manifest.baseline_capabilities,
        resource_profile=instance.manifest.resource_profile,
    )
    instance.trust_boundary_id = proc.trust_boundary_id
    instance.state = BOUND
    events.emit(instance, "bound", intent=instance.bound_intent)

    instance.state = EXECUTING
    while instance.state == EXECUTING:
        step = proc.next_reasoning_step()          # model inference, tool-call proposal

        if step.kind == "capability_call":
            grant = broker.resolve_grant(instance, step.capability_ref, step.scope)
            if grant is PENDING_CONSENT:
                instance.state = WAITING_ON_CAPABILITY
                ui.request_consent(instance, step.capability_ref)   # 13-dynamic-ui-runtime.md
                grant = wait_for_decision(instance)                  # user or policy resolves

            if grant is DENIED:
                proc.deny(step, reason="capability not granted")
                continue

            if not quota.try_consume(instance, step.cost_estimate):  # §6.2 token buckets
                instance.state = WAITING_ON_CAPABILITY
                scheduler.enqueue(instance, priority=instance.priority_class)
                wait_for_scheduler_slot(instance)

            result = scheduler.dispatch(instance, step.capability_ref, step.args, grant)
            proc.resume_with(result)
            audit.record(instance, step, grant, result)             # §5.4
            events.emit(instance, "progress", detail=result.summary) # 31-event-system.md

            if quota.breaker_tripped(instance):
                instance.state = SUSPENDED
                checkpoint_id = checkpoint(instance)
                events.emit(instance, "suspended_runaway", checkpoint_id=checkpoint_id)
                return

            instance.state = EXECUTING

        elif step.kind == "done":
            instance.state = COMPLETED
        elif step.kind == "error":
            instance.state = FAILED

    events.emit(instance, instance.state.name.lower())
    audit.finalize(instance)
```

## 9. Security Considerations

Every invariant in [02 §4](02-core-architecture.md#4-design-invariants) applies directly: **no
silent authority** — grants are single-scope, expiring tokens (§5.3), never ambient; **everything
undoable or versioned** — Capability side effects on Semantic Objects go through
[33 — Rollback & Recovery](33-rollback-recovery.md) checkpoints before an Agent's write commits;
**local-first** — an Agent's resource profile records whether its bound Capabilities may
execute remotely, and that choice is never made silently (see
[22 — Local AI Runtime](22-local-ai-runtime.md)). Sibling Agent instances, even within the same
[12 — Multi-Agent Coordination](12-multi-agent-coordination.md) team, sit behind **separate Trust
Boundaries**: one Agent cannot read another's Context Bundle, reasoning state, or grants except
through the explicit, audited channels [12](12-multi-agent-coordination.md) defines. This blocks
the most common multi-agent security failure mode — one compromised or manipulated Agent
"borrowing" a sibling's authority — at the architecture level rather than relying on each Agent's
own good behavior. Manifests are signed (§5.1) and trust-tiered so a malicious or buggy
third-party specialization cannot self-declare `system`-tier baseline Capabilities; the Broker
re-verifies trust tier on every grant resolution, not only at install time.

## 10. Failure Modes

- **Runaway Agent** — infinite reasoning loop hammering a Capability. Contained by the circuit
  breaker and token buckets (§6.2); cannot starve other Agents due to fair-share queuing.
- **Capability grant deadlock** — an Agent blocks indefinitely awaiting user consent while the
  user is away. Mitigated by a bounded `waiting_on_capability` timeout that auto-suspends and
  checkpoints rather than holding resources forever.
- **Mid-invocation crash** — the sandboxed process dies while a Capability write is in flight,
  risking a half-written Semantic Object. Prevented by requiring Capability side effects to be
  transactional against [33 — Rollback & Recovery](33-rollback-recovery.md) recovery points.
- **Hallucinated Capability reference** — the Agent's reasoning proposes a Capability that does
  not exist or that it was never granted. The Broker's deny path (§6.1) makes this a no-op, logged
  and surfaced, not a crash.
- **Checkpoint corruption** — a serialized state that fails to deserialize on resume. Resume falls
  back to the last valid prior checkpoint, or to a cold re-bind of the original Intent and Context
  Bundle if none exist.
- **Watchdog false positive** — a legitimately slow but healthy Agent (e.g., a long web research
  task) gets suspended by an overly aggressive timeout. Mitigated by specialization-specific
  wall-clock defaults in the resource profile rather than one global timeout.

## 11. Recovery Mechanisms

The Agent Supervisor maintains a heartbeat per instance; missed heartbeats beyond a
specialization-tuned threshold trigger the same suspend-and-checkpoint path as a quota breach.
Supervisors restart failed Agents with exponential backoff up to a retry ceiling, after which the
failure is escalated as a user-visible event rather than retried silently — directly implementing
[01 — Vision & Philosophy §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable)'s
requirement that autonomous action be observable and interruptible even when it fails. Every
suspended or failed instance's Execution Record (§5.4) is replayable through
[18 — Explainability & Trust](18-explainability-and-trust.md) for diagnosis, and its last good
checkpoint is a valid [33 — Rollback & Recovery](33-rollback-recovery.md) restore point, so a
crashed Agent never leaves the Intent it was working on in an undefined state — the Intent Graph
node reverts to `planned` and can be reassigned.

## 12. Performance Analysis

Cold spawn (sandbox creation, manifest resolution, baseline grant issuance) is budgeted at
low-tens-of-milliseconds for `process`-class manifests and higher for `container`/`microvm`
tiers used by lower-trust specializations; a warm-instance pool keyed by `(specialization,
trust_tier)` amortizes this for latency-sensitive interactive Agents, consistent with the
sub-second Workspace targets in [36 — Performance Benchmarks](36-performance-benchmarks.md).
Steady-state overhead is dominated by the Broker's grant-resolution round trip, which is why
baseline (pre-approved) Capabilities exist at all — they remove the common-case grant check from
the hot path. Concurrent-Agent scaling is governed by the fair-share queue in §6.2 and degrades
gracefully under load per [37 — Scalability Roadmap](37-scalability-roadmap.md): on constrained
hardware (e.g., a Raspberry Pi-class device), the Scheduler shrinks the number of `executing`
instances rather than shrinking any single Agent's correctness guarantees, pushing excess demand
into `waiting_on_capability` queues instead of failure.

## 13. Trade-offs

- **Process/container-per-Agent isolation** costs spawn latency and memory versus a
  thread-per-Agent model, but a shared address space would violate the Trust Boundary guarantee
  in [02 §2](02-core-architecture.md#2-shared-vocabulary) the moment any Agent handled untrusted
  input — isolation was chosen over raw efficiency, consistent with the Golden Rule from
  [01 §2](01-vision-and-philosophy.md#2-the-golden-rule) (a faster but less trustworthy Agent does
  not make the user's goal easier to accomplish safely).
- **Static baseline manifests vs. fully dynamic capability requests** — baseline grants trade some
  flexibility for a much smaller consent-prompt surface and a faster hot path; the
  `requestable_capabilities` escape hatch (§6.1) recovers flexibility at the cost of an explicit
  grant round trip, which is the correct default per [02's "no silent authority"](02-core-architecture.md#4-design-invariants).
- **Centralized Capability Broker** is a single, thoroughly auditable enforcement point (good for
  [18](18-explainability-and-trust.md) and [15](15-security-architecture.md)) but is a potential
  throughput bottleneck at very high Agent concurrency; it is designed to be horizontally
  replicated with a shared, versioned grant ledger rather than becoming a single point of failure.

## 14. Testing Strategy

Unit tests cover every lifecycle transition in §3.3, including all abort-to-`terminated` edges.
Fuzz testing targets the manifest parser and the Capability Broker's grant-resolution logic (§6.1)
with malformed and adversarial manifests. Chaos tests kill an Agent's sandboxed process at each
point in the pseudocode loop (§8) and assert the Semantic Object under write is never left
half-committed, per [33](33-rollback-recovery.md). Load tests spin up hundreds of concurrent Agent
instances per user session to validate the fair-share queue and circuit breaker under contention.
Security red-team tests specifically attempt Capability escalation (requesting ungranted
Capabilities, forging scope, replaying expired tokens) and cross-instance Trust Boundary crossings
between sibling Agents. Each built-in specialization in §4 additionally carries a golden-trace
regression test asserting its one-line contract holds against a fixed Intent/Context Bundle
fixture, integrated with [35 — Testing Strategy](35-testing-strategy.md).

---
*Next: [12 — Multi-Agent Coordination](12-multi-agent-coordination.md) builds teams of Agent
Runtime instances on top of this substrate to decompose and execute complex, multi-part goals.*
