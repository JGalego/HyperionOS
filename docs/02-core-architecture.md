# Core Architecture

This document defines the layered system architecture and the shared vocabulary used by every
other document in this specification. If a later document uses a capitalized term (`Intent`,
`Capability`, `Semantic Object`, `Context Bundle`, `Workspace`, `Agent`, `Trust Boundary`), its
authoritative definition is here. See [01 — Vision & Philosophy](01-vision-and-philosophy.md) for
why these abstractions exist; see [00 — Index](00-index.md) for the full document map.

## 1. Layered System View

Hyperion is organized into seven layers. Lower layers are more traditional-OS-like and more
deterministic; higher layers are more intent-native and more probabilistic. Every layer exposes a
**capability-secured** interface to the layer above (see §5 and
[15 — Security Architecture](15-security-architecture.md)) — no layer is permitted to reach past
its immediate neighbor.

```
┌─────────────────────────────────────────────────────────────────────┐
│ L6  Experience Layer      Dynamic UI Runtime · Workspaces · Voice/  │
│                           Conversational Shell · Accessibility      │
├─────────────────────────────────────────────────────────────────────┤
│ L5  Coordination Layer    Multi-Agent Orchestration · Workflow      │
│                           Execution · Explainability & Audit Log    │
├─────────────────────────────────────────────────────────────────────┤
│ L4  Cognition Layer       Intent Engine · Context Engine · Memory   │
│                           Engine · Agent Runtime · Model Router     │
├─────────────────────────────────────────────────────────────────────┤
│ L3  Knowledge Layer       Knowledge Graph · Semantic Filesystem ·   │
│                           Semantic Object Store                     │
├─────────────────────────────────────────────────────────────────────┤
│ L2  Platform Services     Capability Registry · Plugin Framework ·  │
│                           Storage Engine · Event System · Update    │
│                           System · Networking Stack                 │
├─────────────────────────────────────────────────────────────────────┤
│ L1  System Runtime        Scheduler · IPC Framework · Sandboxing ·  │
│                           Container/VM Runtime · Device Framework   │
├─────────────────────────────────────────────────────────────────────┤
│ L0  Kernel                HAL · Driver Model · Capability Security ·│
│                           Process/Memory Primitives · GPU Scheduling│
└─────────────────────────────────────────────────────────────────────┘
```

Each layer's document is the authority for that layer's internals; this document is the authority
for how they compose. A useful reading order is top-down (start from what the user experiences)
or bottom-up (start from what the hardware provides) — [00 — Index](00-index.md) supports both.

## 2. Shared Vocabulary

### Intent
A structured, evolving representation of a goal the user has expressed, in natural language or
otherwise. An Intent is never a single command — it is a node in an **Intent Graph**, a directed
graph of sub-goals, dependencies, and status, maintained by the
[Intent Engine](05-intent-engine.md). Intents have a lifecycle (`proposed → planned → executing →
{completed, abandoned, superseded}`) and are always attributable to a human request, even when
decomposed automatically into many sub-intents.

### Capability
The unit of installable, invokable functionality — Hyperion's replacement for the application.
A Capability declares:
- a semantic **contract** (inputs, outputs, side effects, required permissions)
- zero or more **implementations** (a local model, a cloud API, a native binary, another
  Capability composed from others)
- a **trust level** and **resource profile** used by the [Scheduler](04-scheduler.md) and
  [Security Architecture](15-security-architecture.md)

The OS — not the user, and not the developer — chooses which implementation of a Capability
satisfies a given Intent at a given moment, based on context, privacy settings, and available
resources (see [24 — Plugin Framework](24-plugin-framework.md) and
[25 — SDK](25-sdk.md)).

### Semantic Object
The universal unit of stored information, replacing the file. Every document, photo, video,
message, person, meeting, project, task, company, code repository, or piece of knowledge in
Hyperion is a Semantic Object with:
- stable identity (an Object ID, independent of any name or path)
- typed relationships to other objects (forming the [Knowledge Graph](09-knowledge-graph.md))
- metadata, permissions, semantic embeddings, version history, and reasoning provenance

Folders and filenames are a compatibility *view* over Semantic Objects, generated on demand by
the [Semantic Filesystem](10-semantic-filesystem.md) — they are not the underlying storage model
(see [28 — Storage Engine](28-storage-engine.md)).

### Context Bundle
The minimal, task-scoped slice of state — active Semantic Objects, recent Intents, relevant
Memory, device/session identity — that Hyperion attaches to an Intent or Agent invocation so
that "continue yesterday's work" resolves without the user re-stating anything. Produced and
propagated by the [Context Engine](06-context-engine.md); the wire format and propagation rules
are defined in [07 — Context Propagation](07-context-propagation.md).

### Workspace
A UI surface synthesized for the duration of a goal by the
[Dynamic UI Runtime](13-dynamic-ui-runtime.md), composed of Capabilities and bound to the
Semantic Objects relevant to the active Intent. Workspaces are ephemeral by default: they are
generated, used, and torn down; a user may pin one to persist it, which converts it into a durable
Semantic Object.

### Agent
A specialized reasoning process — Research, Coding, Writing, Design, Travel, Security, and others
enumerated in [11 — Agent Runtime](11-agent-runtime.md) — that consumes Intents and Context
Bundles and invokes Capabilities to make progress. Agents are scheduled, sandboxed, and audited
exactly like any other computation (see [04 — Scheduler](04-scheduler.md) and
[15 — Security Architecture](15-security-architecture.md)); "agent" is a role a process plays,
not a privileged primitive.

### Trust Boundary
A capability-security enforced boundary (process, container, VM, or remote host) across which no
implicit authority crosses — every crossing requires an explicit, auditable capability grant. See
[03 — Kernel Architecture](03-kernel-architecture.md) and
[15 — Security Architecture](15-security-architecture.md).

## 3. How a Request Flows Through the Layers

Worked trace for "Help me prepare for tomorrow's interview," referenced throughout this
specification:

1. **L6** Conversational shell captures the utterance, attaches device/session identity.
2. **L4** [Intent Engine](05-intent-engine.md) parses it into an Intent Graph (research the
   company, review the job description, rehearse answers, plan logistics). [Context
   Engine](06-context-engine.md) attaches the Context Bundle (calendar event "Interview — Acme
   Corp, 9:00 AM", the stored resume, prior interview notes from [Memory](08-memory-engine.md)).
3. **L5** The Intent Graph is handed to [Multi-Agent Coordination](12-multi-agent-coordination.md),
   which assigns sub-intents to a Research Agent, a Scheduling Agent, and a Coaching Agent.
4. **L4** Each Agent resolves its sub-intent by invoking Capabilities (`web.research`,
   `calendar.read`, `document.summarize`) chosen and routed by the
   [Model Router](23-multi-model-orchestration.md).
5. **L3** Capabilities read and write Semantic Objects (the job posting, the resume, new
   flashcard notes) through the [Knowledge Graph](09-knowledge-graph.md).
6. **L2/L1/L0** The Scheduler places each Capability invocation on the resource (local NPU,
   local CPU, or a consented cloud call) that meets its latency and privacy requirements; the
   Kernel enforces the Trust Boundary around every invocation.
7. **L6** The [Dynamic UI Runtime](13-dynamic-ui-runtime.md) assembles a "Prepare for Interview"
   Workspace (notes, flashcards, a timer, the job posting) as results land, with every step
   explainable per [18 — Explainability & Trust](18-explainability-and-trust.md).

Every subsystem document in this specification should be read as "what layer N does in step M of
a trace like this one."

## 4. Design Invariants

These invariants bind every subsystem and are checked against in each document's Trade-offs
section:

1. **No silent authority.** Nothing crosses a Trust Boundary without an explicit, revocable
   capability grant (see [15 — Security Architecture](15-security-architecture.md)).
2. **Everything is undoable or versioned.** State-changing operations produce a recovery point
   before they execute (see [33 — Rollback & Recovery](33-rollback-recovery.md)).
3. **Local-first by default.** Computation and storage prefer the local device; cloud/remote
   execution is an explicit, consented upgrade, never a silent fallback (see
   [22 — Local AI Runtime](22-local-ai-runtime.md) and
   [16 — Privacy Architecture](16-privacy-architecture.md)).
4. **Every autonomous action is explainable on demand** (see
   [18 — Explainability & Trust](18-explainability-and-trust.md)).
5. **Degrade, never fail closed on the user's goal.** If an ideal Capability implementation is
   unavailable, the system substitutes a lesser one and says so, rather than blocking the user
   (see [37 — Scalability Roadmap](37-scalability-roadmap.md)).
6. **Accessibility is not a mode.** Every Workspace generated by L6 must satisfy
   [14 — Accessibility](14-accessibility.md) constraints unconditionally.

## 5. Capability Security as the Unifying Security Model

Every layer boundary in §1, every Agent invocation, every Plugin, and every cross-device call
uses the same primitive: an unforgeable **capability token** scoped to one object or one
operation, with an expiry and an audit record. There is exactly one security model in Hyperion,
enforced at the kernel boundary and re-checked at every layer above it — not a kernel permission
model plus an unrelated application permission model plus an unrelated cloud IAM model. Full
treatment in [03 — Kernel Architecture](03-kernel-architecture.md) §Capability Security and
[15 — Security Architecture](15-security-architecture.md).

## 6. Document Map

See [00 — Index](00-index.md) for the complete, annotated table of contents to every subsystem
document, organized by layer, plus the [10-phase implementation plan](41-implementation-phases.md)
that builds these layers incrementally starting from L0.

---
*Next: [03 — Kernel Architecture](03-kernel-architecture.md).*
