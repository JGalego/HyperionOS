# Intent Engine

## Purpose

The Intent Engine is the L4 subsystem (see the layer table in
[02 — Core Architecture](02-core-architecture.md#1-layered-system-view)) that turns raw,
underspecified human expression — typed text, speech, a screen selection, a photo of a whiteboard
— into a structured **Intent Graph**: a directed graph of **Intent** nodes with dependencies,
priorities, and lifecycle status that the rest of the system can plan against, schedule, execute,
audit, and undo. It is the first place in Hyperion where "what the human said" becomes "what the
machine will do," and it is therefore the component most directly answerable to the Golden Rule in
[01 — Vision & Philosophy](01-vision-and-philosophy.md#2-the-golden-rule): every parse, every
decomposition, every inference must make accomplishing the stated goal easier, not merely more
automatable.

This document is the deep-dive on step 2 of the worked trace in
[02 — Core Architecture §3](02-core-architecture.md#3-how-a-request-flows-through-the-layers) — the
moment "Help me prepare for tomorrow's interview" becomes an Intent Graph of sub-goals before it is
handed to [12 — Multi-Agent Coordination](12-multi-agent-coordination.md).

## Motivation

A traditional OS requires the user to already know the decomposition of their goal: which
application to open, which menu to use, which file to attach. [01 — Vision &
Philosophy §5](01-vision-and-philosophy.md#5-universal-usability-highest-priority) rejects this —
the user should never have to know that "launch my startup" implies market research, a website,
legal formation, and a financial model. Someone, or something, has to perform that decomposition.
Hyperion's answer is a first-class Intent Engine rather than an ad-hoc prompt-to-tool-call layer,
for three reasons:

1. **Intents must persist and evolve.** A goal expressed on Monday ("prepare for the interview")
   is still live on Tuesday morning, can be paused, resumed, amended, or cancelled, and must
   survive across devices ([21 — Distributed Execution](21-distributed-execution.md)) and app
   sessions. A stateless prompt-to-action mapping cannot do this; a persistent graph can.
2. **Decomposition must be auditable and reversible.** Per [01 §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable),
   every autonomous step must be explainable, interruptible, and undoable. That requires a
   structured plan artifact — the Intent Graph — not an opaque chain-of-thought.
3. **Ambiguity is the norm, not the exception**, in natural language. "Continue working on the
   API" and "actually, cancel that" are only resolvable against structured state: prior Intents,
   Context, and Memory. A one-shot parser without a persistent graph and without
   [06 — Context Engine](06-context-engine.md) and [08 — Memory Engine](08-memory-engine.md) to
   consult has nothing to disambiguate against.

## Architecture

```
                    ┌───────────────────────────────────────────────────────────┐
                    │                    L6 Experience Layer                    │
                    │      (utterance, gesture, selection, or file drop)        │
                    └───────────────────────────┬───────────────────────────────┘
                                                 │ raw multimodal input
                                                 ▼
 ┌───────────────────────────────────────────────────────────────────────────────────┐
 │                              INTENT ENGINE  (L4)                                  │
 │                                                                                     │
 │   ┌───────────────┐    ┌────────────────┐    ┌───────────────────────────────┐    │
 │   │ Capture &     │    │  Grounding     │    │        Intent Compiler        │    │
 │   │ Normalization │───▶│  Resolver      │───▶│  (canonical goal predicate +  │    │
 │   │ (ASR, OCR,    │    │  (entity/slot  │    │   typed slots + constraints)  │    │
 │   │  segmentation)│    │  disambiguation)│    └───────────────┬────────────────┘    │
 │   └───────────────┘    └───────┬────────┘                    │                     │
 │                                │  reads                      ▼                     │
 │                    ┌───────────┴──────────┐      ┌─────────────────────────┐       │
 │                    │  06 Context Engine    │      │   Decomposition Planner │       │
 │                    │  08 Memory Engine     │◀────▶│  (HTN templates +       │       │
 │                    │  09 Knowledge Graph   │      │   generative planning)  │       │
 │                    └───────────────────────┘      └───────────┬─────────────┘       │
 │                                                                │ builds              │
 │                                                                ▼                     │
 │   ┌───────────────────────┐   consults    ┌─────────────────────────────────┐       │
 │   │  Ambiguity Resolver    │◀─────────────▶│         INTENT GRAPH            │       │
 │   │ (ask vs. infer policy) │               │  (nodes, edges, status, prio)   │       │
 │   └───────────┬───────────┘               └───────────────┬─────────────────┘       │
 │               │ clarifying question                        │ mutations               │
 │               ▼                                             ▼                        │
 │   ┌────────────────────┐                     ┌───────────────────────────────┐       │
 │   │   User (L6 shell)   │                     │     Reconciliation Engine     │       │
 │   └────────────────────┘                     │ ("actually cancel that")      │       │
 │                                               └───────────────┬───────────────┘       │
 └───────────────────────────────────────────────────────────────┼───────────────────────┘
                                                                  │ submit(graph)
                                                                  ▼
                          ┌─────────────────────────────────────────────────────┐
                          │      12 — Multi-Agent Coordination (L5)             │
                          │   assigns sub-intents to Agents, schedules work     │
                          └─────────────────────────────────────────────────────┘
```

The engine is deliberately split into a **fast path** (Capture → Grounding → Compiler, target
sub-200ms for common cases, see [36 — Performance Benchmarks](36-performance-benchmarks.md)) and a
**slow path** (Decomposition Planner for novel or deep goals), so the user gets an immediate
acknowledgment ("Planning your interview prep...") while deeper decomposition streams in.

## Data Structures

```
Intent {
  id: IntentID                          // stable, globally unique
  statement: {
    raw_utterance: string,              // verbatim source, for audit/explainability
    predicate: GoalPredicate,           // canonical structured form, e.g. research(target=Company)
    slots: [Slot],
  }
  parent_id: IntentID | null
  children: [IntentID]
  depends_on: [IntentID]                // hard prerequisite edges
  informs: [IntentID]                   // soft/data-sharing edges, not blocking
  status: proposed | planned | executing | completed | abandoned | superseded   // 02 §2
  priority: float                        // derived, see Algorithms §3
  confidence: float                      // grounding/decomposition confidence, 0..1
  capability_hints: [CapabilityRef]      // candidate Capabilities that could satisfy this leaf
  grounded_entities: [SemanticObjectRef] // see 09 — Knowledge Graph
  assigned_agent: AgentRef | null        // set once 12 accepts the node
  provenance: {
    created_at, updated_at,
    inferred_fields: [FieldProvenance],  // what was asked vs. inferred, and from where
  }
}

Slot {
  name: string
  value: SemanticObjectRef | Literal | null
  grounding_status: grounded | ambiguous | missing
  candidates: [(SemanticObjectRef, confidence)]
}

IntentGraph {
  graph_id: GraphID
  root: IntentID
  nodes: Map<IntentID, Intent>
  edges: [Edge]                          // typed: depends_on | informs | supersedes | alternative_to
  version: MonotonicCounter               // bumped on every mutation, read by 07 — Context Propagation
  workspace_ref: WorkspaceID | null       // bound once 13 — Dynamic UI Runtime materializes a Workspace
}

GraphMutation {                          // append-only edit log entry, powers Reconciliation + 33
  op: cancel | amend | reprioritize | pause | resume | supersede
  target: IntentID
  patch: object
  triggered_by_utterance: string | null
  timestamp
}
```

The graph is a DAG for `depends_on`/`informs` edges but permits `supersedes` back-references, since
a superseding Intent must point at what it replaces for audit purposes (see
[18 — Explainability & Trust](18-explainability-and-trust.md)).

## Algorithms

**1. Parse & ground.** Tokenize/transcribe the input, classify it against a goal taxonomy (a
learned classifier, not a fixed command list — open-vocabulary by design), extract slots, then
resolve each slot's candidate value against [09 — Knowledge Graph](09-knowledge-graph.md) entities
and recent [08 — Memory Engine](08-memory-engine.md) references, scored by a grounding confidence
function combining string/semantic similarity, recency, and explicit-mention strength.

**2. Decompose.** Known goal shapes (business formation, trip planning, interview prep, code
change) are decomposed via Hierarchical Task Network templates maintained as versioned Semantic
Objects themselves — editable, auditable, improvable over time. Novel or compound goals fall back
to generative decomposition: a planning model proposes a sub-intent graph, which is then validated
against the Capability Registry ([02 §"Capability"](02-core-architecture.md#capability)) — every
leaf must have at least one candidate Capability chain, or the leaf is flagged `unsatisfiable` and
surfaced to the user rather than silently dropped (Design Invariant 5 in
[02 §4](02-core-architecture.md#4-design-invariants)).

**3. Prioritize.** Priority is not user-assigned by default; it is derived from (a) explicit
deadlines pulled from the Context Bundle (e.g., a calendar event), (b) position on the critical
path of the dependency graph, and (c) explicit urgency language in the utterance. Priority is
continuously recomputed as sibling Intents complete or slip, not fixed at planning time.

**4. Resolve ambiguity: ask vs. infer.** For every `ambiguous` or `missing` slot, the Ambiguity
Resolver computes `expected_cost_of_wrong_inference` (how expensive/irreversible is acting on the
wrong binding — see [15 — Security Architecture](15-security-architecture.md) trust level of the
implicated Capability) against `cost_of_interrupting_user` (roughly: how confident is the top
candidate, how many clarifications has the user already answered this session). Below a
configurable confidence floor, or above an irreversibility threshold, the engine asks. Otherwise it
infers, silently records the inference in `provenance.inferred_fields`, and keeps the door open —
an inferred field is always the *cheapest thing to correct* in the graph, never load-bearing for an
action that cannot be undone (Design Invariant 2, [02 §4](02-core-architecture.md#4-design-invariants)).

**5. Reconcile.** A follow-up utterance is first tested for **reference resolution** against active
graphs (most-recently-touched graph, or an explicit anaphor like "that" / "the website one") using
the same grounding machinery as step 1. A resolved reference becomes a `GraphMutation` rather than
a new Intent: `cancel` marks the target and its unshared descendants `abandoned` and notifies
[12 — Multi-Agent Coordination](12-multi-agent-coordination.md) to halt any executing Agent
cleanly; `amend` patches a slot and re-triggers decomposition only for the affected subtree, not
the whole graph; `supersede` creates a new Intent, links `supersedes → old`, and marks the old
`superseded`, preserving history rather than overwriting it.

**6. Detect conflicts.** Before submission, the engine checks the new/mutated subtree against all
other `executing` Intents for exclusive-resource or contradictory-goal conflicts (e.g., "cancel the
trip" while a Booking Agent is mid-purchase). Detected conflicts are raised as a blocking
clarifying question rather than resolved by silent priority ordering, per
[01 §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable).

## Interfaces / APIs

```
IntentEngine.parse(utterance, context_bundle) -> Intent
IntentEngine.decompose(intent) -> IntentGraph
IntentEngine.reconcile(utterance, active_graph_ids) -> GraphMutation | NewIntent
IntentEngine.getGraph(graph_id) -> IntentGraph
IntentEngine.submit(graph) -> ExecutionTicket        // consumed by 12 — Multi-Agent Coordination
IntentEngine.explain(intent_id) -> ProvenanceReport   // backs 18 — Explainability & Trust
```

Status transitions and graph mutations are published as `intent.proposed`, `intent.planned`,
`intent.status_changed`, and `intent.superseded` notifications through
[31 — Event System](31-event-system.md) for any interested observer (e.g., a dashboard Workspace);
this is distinct from the point-to-point `submit()` handoff to
[12 — Multi-Agent Coordination](12-multi-agent-coordination.md), which carries the full graph, not
a notification — see [07 — Context Propagation](07-context-propagation.md) for that distinction in
general and its wire format.

## Pseudocode

```python
def handle_utterance(utterance, session):
    ctx = ContextEngine.assemble(intent_or_session=session)       # 06 — Context Engine
    intent = ground_and_compile(utterance, ctx)

    ref = try_resolve_reference(utterance, session.active_graphs) # step 5: reconciliation first
    if ref is not None:
        mutation = build_mutation(utterance, ref, ctx)
        graph = apply_mutation(ref.graph_id, mutation)
        MultiAgentCoordination.notify(graph, mutation)             # 12
        return graph

    graph = decompose(intent)                                     # HTN template or generative
    for leaf in graph.leaves():
        for slot in leaf.statement.slots:
            if slot.grounding_status != "grounded":
                if should_ask(slot, leaf):                         # cost/confidence policy
                    answer = ask_user(slot, leaf)
                    bind(slot, answer)
                else:
                    infer(slot, ctx, MemoryEngine.working_memory(session))  # 08
                    leaf.provenance.inferred_fields.append(slot.name)

    conflicts = detect_conflicts(graph, session.active_graphs)
    if conflicts:
        resolution = ask_user_to_resolve(conflicts)
        apply_resolution(graph, resolution)

    graph.version += 1
    ticket = MultiAgentCoordination.submit(graph)                  # 12 — Multi-Agent Coordination
    return graph, ticket


def apply_mutation(graph_id, mutation):
    graph = load_graph(graph_id)
    match mutation.op:
        case "cancel":
            for node in subtree(graph, mutation.target):
                if not shared_with_other_active_intent(node):
                    node.status = "abandoned"
            MultiAgentCoordination.halt(graph_id, mutation.target)  # 12
        case "amend":
            node = graph.nodes[mutation.target]
            apply_patch(node, mutation.patch)
            resubmit_subtree = decompose(node)                     # re-plan only the changed part
            graph.replace_subtree(mutation.target, resubmit_subtree)
        case "supersede":
            old = graph.nodes[mutation.target]
            old.status = "superseded"
            new = compile_from_patch(old, mutation.patch)
            graph.add_node(new, edges=[("supersedes", old.id)])
    graph.version += 1
    persist(graph)                                                  # versioned, see 33
    return graph
```

## Worked Example

Utterance: **"I need to launch my startup."**

Parse & ground yields a root Intent with predicate `found_company()` and no groundable slots yet
(no company name given — the engine proceeds with a placeholder Semantic Object created for the
still-unnamed venture, per Design Invariant 5: degrade, never block). Decomposition against the
`business-formation` HTN template produces:

```
Intent Graph: "Launch my startup"                              status legend: [P]roposed
                                                                              [L]anned
Root ── found_company()  [status: planned, priority: 1.0]                   [E]xecuting
 │                                                                            [C]ompleted
 ├─(depends_on: none)──▶ 1. Business Planning        [L, prio 0.95]
 │                          │
 │                          ├─(depends_on: none)──▶ 1a. Market Research      [E, prio 0.95]
 │                          │        (assigned: Research Agent — 12)
 │                          └─(depends_on: 1a)─────▶ 1b. Business Model /    [P, prio 0.80]
 │                                                        Financial Planning
 │
 ├─(depends_on: 1a)────▶ 2. Branding                 [P, prio 0.70]
 │                          └─(informs: 3)──────────▶ (feeds naming into Legal)
 │
 ├─(depends_on: 2)─────▶ 3. Legal (entity formation, [P, prio 0.75]
 │                          trademark check)
 │
 ├─(depends_on: 2)─────▶ 4. Website                  [P, prio 0.65]
 │                          (assigned: Design + Coding Agents)
 │
 ├─(depends_on: 3, 1b)─▶ 5. Customer Outreach        [P, prio 0.50]
 │
 ├─(depends_on: 3,4,5)─▶ 6. Execution (launch)       [P, prio 0.40]
 │
 └─(depends_on: 6)─────▶ 7. Continuous Monitoring    [P, prio 0.20, recurring]
```

Only node **1a (Market Research)** starts in `executing` status — it has no dependencies and the
Decomposition Planner marks it critical-path. Every other node is `planned` (fully specified,
capability-feasible, waiting on its dependency) or `proposed` (structure exists, slots still
underspecified — e.g., node 3's jurisdiction slot is `ambiguous` until Market Research narrows the
target market). The whole graph is submitted once via `IntentEngine.submit(graph)` to
[12 — Multi-Agent Coordination](12-multi-agent-coordination.md), which fans nodes 1a onward out to
Agents as their dependencies clear — see that document for the assignment and scheduling policy.

If the user later says **"actually, forget the physical branding, I'm going digital-only,"** the
Reconciliation Engine resolves "the branding" to node 2, applies an `amend` mutation that patches
its predicate from `brand(medium=physical+digital)` to `brand(medium=digital)`, and re-decomposes
only that subtree — nodes 3-7 are untouched except for re-evaluated priority, and node 2's prior
version remains in the audit log per [18 — Explainability & Trust](18-explainability-and-trust.md).

## Security Considerations

Grounding a slot against the Knowledge Graph is a *read* across whatever Trust Boundary
(per [02 §"Trust Boundary"](02-core-architecture.md#trust-boundary)) currently contains the
candidate Semantic Object, and is capability-checked exactly like any other cross-boundary read
(see [15 — Security Architecture](15-security-architecture.md)) — the Intent Engine holds no
ambient authority to read anything it has not been granted. Decomposition must never silently
select a Capability implementation whose trust level exceeds what the originating Intent has been
consented for (e.g., a "financial planning" leaf must not silently invoke a Capability with
bank-write access without an explicit grant). Multimodal capture is a documented prompt-injection
surface — text embedded in an image, or instructions spoken by a second voice in an audio clip —
so Capture & Normalization treats all extracted content as **data**, never as engine instructions,
and any attempt by parsed content to name a Capability directly is rejected and logged rather than
honored (full threat enumeration in [17 — Threat Model](17-threat-model.md)).

## Failure Modes

- **Misgrounding**: a slot binds to the wrong Semantic Object with high confidence (e.g., the
  wrong "the API" repository). Mitigated by confidence-gated confirmation on any leaf whose
  Capability trust level is non-trivial, and by the Ambiguity Resolver's cost-of-being-wrong
  weighting, but not eliminated — see Recovery below.
- **Decomposition explosion**: a generative planner over-decomposes a vague goal into hundreds of
  speculative leaves. Bounded by a per-graph node budget and lazy expansion (only the frontier
  subtree is fully decomposed; deeper subtrees are stubbed until their turn).
- **Dependency deadlock**: a cycle introduced by a bad `amend` (rare, since edges are typed and
  `depends_on` is cycle-checked on every mutation) — detected at commit time and rejected before
  `persist()`.
- **Ambiguous cancellation target**: "cancel that" with two plausible referents. Resolved by
  falling back to a clarifying question rather than guessing, per the ask/infer policy applied to
  reference resolution itself.
- **Prompt injection via captured content**: covered under Security Considerations above.

## Recovery Mechanisms

Every graph mutation is versioned (`IntentGraph.version`) and persisted before any Agent acts on
it, giving [33 — Rollback & Recovery](33-rollback-recovery.md) a clean point to restore to if a
decomposition or inference turns out wrong. A user correction ("no, the *other* API repo") is fed
back into [08 — Memory Engine](08-memory-engine.md) as a disambiguation preference, improving
future grounding confidence for that user without any model retraining. Deadlocks and
unsatisfiable leaves fail the specific node, not the graph — sibling subtrees continue executing
(Design Invariant 5) while the blocked node is surfaced for user input.

## Performance Analysis

Target latencies (see [36 — Performance Benchmarks](36-performance-benchmarks.md) for the
system-wide budget): parse & ground on-device under 150ms for the common case (cached embeddings,
local model per [22 — Local AI Runtime](22-local-ai-runtime.md)); template-based decomposition
under 300ms; generative decomposition for novel goals may exceed this and is streamed
incrementally — the frontier leaf is submitted to [12](12-multi-agent-coordination.md) as soon as
it is ready rather than waiting on the full graph, so the user sees progress within the same budget
as a template-based plan. Graph size is bounded (default 200 nodes/graph) with lazy stub expansion
for larger goals to keep planning cost sub-linear in eventual graph size.

## Trade-offs

- **Template determinism vs. generative flexibility**: templates are auditable, fast, and
  predictable but only cover known goal shapes; generative decomposition covers the long tail but
  is slower and requires stronger validation against the Capability Registry before trust. Hyperion
  runs both, templates first, generative as fallback.
- **Ask vs. infer**: asking preserves correctness and trust but taxes the user's attention, working
  against [01 §5](01-vision-and-philosophy.md#5-universal-usability-highest-priority); inferring
  preserves flow but risks silent misdirection. The cost-weighted policy in Algorithms §4 is the
  chosen balance point, tunable per Capability trust level.
- **Graph granularity**: fine-grained decomposition parallelizes better across Agents but adds
  coordination overhead in [12](12-multi-agent-coordination.md); coarse-grained decomposition is
  cheaper to plan but serializes work. The HTN templates are hand-tuned for this balance; generative
  decomposition is explicitly instructed to prefer the coarser of two equally valid decompositions.

## Testing Strategy

A golden corpus of utterances mapped to expected Intent Graphs (including the startup and
interview examples) regression-tests parsing and decomposition on every change. A separate
adversarial corpus targets reference-resolution ambiguity ("cancel that," "no, the other one") and
prompt-injection payloads embedded in transcribed/OCR'd content. Multi-turn conversation simulation
exercises the full ask/infer/reconcile loop, asserting that inferred fields are always the ones
recorded as such in `provenance`, that cancellation never abandons a node shared by another active
Intent, and that no submitted graph ever contains an `unsatisfiable` leaf without a corresponding
user-facing clarification. Load tests confirm the node-budget/lazy-expansion strategy keeps p99
planning latency bounded as goal complexity grows.

---
*Next: [06 — Context Engine](06-context-engine.md).*
