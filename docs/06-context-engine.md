# Context Engine

## Purpose

The Context Engine is the L4 subsystem (see
[02 — Core Architecture §1](02-core-architecture.md#1-layered-system-view)) responsible for
assembling the **Context Bundle** — defined in
[02 §"Context Bundle"](02-core-architecture.md#context-bundle) as "the minimal, task-scoped slice
of state... that Hyperion attaches to an Intent or Agent invocation" — for every Intent produced by
[05 — Intent Engine](05-intent-engine.md) and every Agent invocation made by
[11 — Agent Runtime](11-agent-runtime.md). It is the reason "Continue working on the API" needs no
further explanation: repository, branch, customer, deadline, team, related issues, recent
discussions, documentation, and deployments are already attached before the sentence is even fully
parsed. This document covers step 2 of the worked trace in
[02 §3](02-core-architecture.md#3-how-a-request-flows-through-the-layers) alongside
[05 — Intent Engine](05-intent-engine.md); the format that Context Bundle takes when it crosses a
process, device, or Trust Boundary is specified separately in
[07 — Context Propagation](07-context-propagation.md).

## Motivation

[01 — Vision & Philosophy §7](01-vision-and-philosophy.md#7-human-language-first) requires that
Hyperion "resolve the ambiguity using Context, Memory, and the Knowledge Graph, rather than asking
the user to disambiguate up front." That is only possible if something is constantly, cheaply
assembling a working picture of "what's relevant right now" before the user asks. Three
requirements fall out of this:

1. **It must be automatic.** The user must never be asked to attach a repository, ticket, or
   customer manually — see the "the API" example in the Algorithms section below.
2. **It must be bounded.** A naive implementation that concatenates every Semantic Object the user
   has ever touched fails on latency, cost, precision (the model reasons worse over irrelevant
   context), and — critically — privacy (see Security Considerations). Ranking and pruning are not
   an optimization here; they are the whole problem.
3. **It must be the substrate for Adaptive Complexity**, per
   [01 §6](01-vision-and-philosophy.md#6-adaptive-complexity): "adaptive complexity is a *read* of
   accumulated procedural and episodic memory, not a separate subsystem." The Context Engine is
   where that read happens.

## Architecture

```
                    ┌───────────────────────────────────────────────────────┐
                    │   Trigger: new Intent (05) or Agent invocation (11)    │
                    └───────────────────────────┬─────────────────────────────┘
                                                 ▼
 ┌───────────────────────────────────────────────────────────────────────────────────────┐
 │                              CONTEXT ENGINE  (L4)                                     │
 │                                                                                         │
 │   ┌─────────────────────────────  Signal Collectors  ─────────────────────────────┐   │
 │   │  Active Semantic    Intent      Device/Session   Calendar &    Recent Agent    │   │
 │   │  Objects (working    History     State (app in    Comms        Activity Log    │   │
 │   │  set, 09/10)         (05)        focus, location) Connectors    (11)           │   │
 │   └─────────┬──────────────┬─────────────┬───────────────┬───────────────┬────────┘   │
 │             ▼              ▼             ▼               ▼               ▼            │
 │        ┌──────────────────────────────────────────────────────────────────────┐       │
 │        │                     Candidate Context Pool                          │       │
 │        └───────────────────────────────┬──────────────────────────────────────┘       │
 │                                         ▼                                              │
 │        ┌────────────────────┐   consults    ┌──────────────────────────────┐         │
 │        │  Entity Resolver    │◀─────────────▶│  09 Knowledge Graph          │         │
 │        │ ("the API" → repo X)│               │  08 Memory Engine (episodic/ │         │
 │        └──────────┬──────────┘               │  working memory)            │         │
 │                   ▼                            └──────────────────────────────┘         │
 │        ┌──────────────────────────────────────────────────────────────────────┐       │
 │        │              Relevance Ranker (recency · affinity · explicit         │       │
 │        │              mention · trust scope · graph distance)                 │       │
 │        └───────────────────────────────┬──────────────────────────────────────┘       │
 │                                         ▼                                              │
 │        ┌──────────────────────────────────────────────────────────────────────┐       │
 │        │        Bundle Assembler (top-K per category + hard budget cap)        │       │
 │        └───────────────────────────┬───────────────────────┬──────────────────┘       │
 │                                     ▼                       ▼                          │
 │                          ┌──────────────────┐   ┌───────────────────────────┐         │
 │                          │  CONTEXT BUNDLE   │   │  Expertise Signal          │         │
 │                          │  (attached to     │   │  (derived, read-only) ───▶ │ 13 UI   │
 │                          │  Intent/Agent)    │   │  feeds Adaptive Complexity │         │
 │                          └──────────┬────────┘   └───────────────────────────┘         │
 └─────────────────────────────────────┼───────────────────────────────────────────────────┘
                                        ▼
                     07 — Context Propagation (wire format, cross-boundary rules)
```

## Data Structures

```
ContextBundle {
  bundle_id: BundleID
  scope: { intent_id | agent_invocation_id }
  entries: [ContextEntry]
  assembled_at: timestamp
  budget: { max_tokens: int, max_entries_per_category: int }
  expertise_signal: ExpertiseEstimate           // read by 13 — Dynamic UI Runtime
}

ContextEntry {
  category: repository | person | deadline | discussion | document | deployment | device | calendar
  ref: SemanticObjectRef                        // 09 — Knowledge Graph
  inclusion_mode: full | reference | summary     // see Algorithms §2 (bounding)
  relevance_score: float
  source_signal: [SignalID]                     // provenance for explainability, 18
  staleness: { generation: int, captured_at: timestamp }  // consumed by 07
}

RelevanceVector {                                // features scored per candidate
  recency: float
  explicit_mention_strength: float               // did the utterance name this directly?
  graph_distance: int                            // hops from an already-anchored entity, 09
  interaction_frequency: float
  trust_scope_match: bool                        // does requester's Trust Boundary permit inclusion?
}

ExpertiseEstimate {
  domain: string                                 // e.g. "coding", "finance", "photo-editing"
  level: novice | intermediate | advanced | expert
  evidence: [ProvenanceRef]                       // vocabulary, capability tier used, error-recovery pattern
  confidence: float
}
```

## Algorithms

**1. Entity resolution ("the API").** When an Intent or utterance references an entity by
underspecified name, the Context Engine does not treat this as a Knowledge Graph lookup in
isolation — it constrains the lookup by the Candidate Context Pool already assembled for the
current session (recently active repositories, the Workspace currently open, the Agent that most
recently touched a "repository"-typed Semantic Object). "The API" resolves to a specific repository
Semantic Object by intersecting: (a) [09 — Knowledge Graph](09-knowledge-graph.md) candidates typed
`repository` whose name or description fuzzy-matches "API", (b) recency in
[08 — Memory Engine](08-memory-engine.md)'s working memory for this session, and (c) affinity to
other already-anchored entities (the customer mentioned three messages ago, the deadline on the
calendar). Ties below a confidence threshold are escalated to the Ambiguity Resolver described in
[05 — Intent Engine §Algorithms](05-intent-engine.md#algorithms), since resolving "the API" is
itself a grounding operation shared with Intent parsing.

**2. Ranking and bounding.** Every candidate in the pool receives a `RelevanceVector` and a scalar
score, a weighted combination biased toward recency and explicit mention, with a graph-distance
decay term (entities more than 2-3 hops from an anchored entity are heavily discounted). The
Bundle Assembler then applies a **hard budget** — by token count, not just entry count — because
"rank everything, include the top N" still fails if N full documents blow the budget. Bounding uses
three inclusion modes rather than a single cutoff:
- `full`: small, highly-ranked entries (a ticket title, a deadline) are inlined.
- `summary`: large but relevant entries (a long discussion thread, a design doc) are inlined as a
  pre-computed semantic summary, not the raw content.
- `reference`: lower-ranked but plausibly-relevant entries are included as a pointer only — an
  Agent that actually needs the full object calls back through
  [09 — Knowledge Graph](09-knowledge-graph.md) to expand it lazily.

This means the bundle is never a data dump; it degrades gracefully from full content to references
as relevance falls off, keeping the total bounded regardless of how much history exists.

**3. Working set maintenance.** The Context Engine does not recompute a bundle from scratch on
every turn. It maintains a per-session **working set** (a ranked, continuously-updated subset of
the Candidate Context Pool) and produces new bundles as incremental diffs against it — an entity
that was relevant three turns ago and hasn't been touched since decays out of the working set
rather than requiring an explicit eviction pass.

**4. Adaptive Complexity read.** On every bundle assembly, the engine also emits an
`ExpertiseEstimate` per relevant domain, computed from signals already flowing through the working
set: vocabulary complexity of recent Intents (from [05](05-intent-engine.md)), the Capability tier
the user has been reaching for (raw API vs. guided workflow, see [25 — SDK](25-sdk.md) and
[26 — APIs](26-apis.md)), and error-recovery behavior (does the user self-correct with technical
vocabulary, or ask Hyperion to explain?). This is deliberately *not* a separate model or a stored
"mode" flag — it is a derived read over the same episodic/procedural memory
([08 — Memory Engine](08-memory-engine.md)) already assembled for context, satisfying
[01 §6](01-vision-and-philosophy.md#6-adaptive-complexity)'s requirement that adaptive behavior be
transparent (`explain()` below) and reversible (a low-confidence or explicitly-overridden estimate
never sticks).

## Interfaces / APIs

```
ContextEngine.assemble(intent_or_invocation, budget?) -> ContextBundle
ContextEngine.resolveEntity(mention, session_pool) -> (SemanticObjectRef, confidence)
ContextEngine.expand(bundle, ref) -> FullSemanticObject      // lazy fetch for `reference` entries
ContextEngine.explain(bundle_id) -> ProvenanceReport          // "why is this in context?", 18
ContextEngine.currentExpertise(domain) -> ExpertiseEstimate   // consumed by 13 — Dynamic UI Runtime
```

The engine subscribes to writes from [08 — Memory Engine](08-memory-engine.md) and updates from
[09 — Knowledge Graph](09-knowledge-graph.md) to keep the working set current, and publishes
lightweight `context.invalidated` notices (bundle id + reason, not the bundle payload) through
[31 — Event System](31-event-system.md) when an included Semantic Object changes underneath an
already-issued bundle — the full staleness-handling contract for a bundle already in flight belongs
to [07 — Context Propagation](07-context-propagation.md).

## Pseudocode

```python
def assemble(scope, budget=DEFAULT_BUDGET):
    working_set = session_working_set(scope.session_id)           # maintained incrementally

    candidates = []
    candidates += working_set.active_semantic_objects()             # 09 / 10
    candidates += IntentEngine.recent_history(scope.session_id, n=10)  # 05
    candidates += device_session_state(scope.device_id)
    candidates += calendar_and_comms_signals(scope.user_id)         # consent-gated, 16

    if scope.mentions:                                              # e.g. "the API"
        for mention in scope.mentions:
            resolved, confidence = resolve_entity(mention, working_set)
            if confidence < DISAMBIGUATION_FLOOR:
                escalate_to_ambiguity_resolver(mention)              # 05 — Intent Engine
            else:
                candidates.append(resolved)

    scored = [(c, score(c, working_set)) for c in candidates]
    scored.sort(key=lambda cs: cs[1], reverse=True)

    entries, tokens_used = [], 0
    for candidate, relevance in scored:
        mode = "full" if relevance > FULL_THRESHOLD and is_small(candidate) \
               else "summary" if relevance > SUMMARY_THRESHOLD \
               else "reference"
        entry = build_entry(candidate, mode, relevance)
        cost = estimate_tokens(entry)
        if tokens_used + cost > budget.max_tokens:
            break                                                    # hard bound, never exceeded
        entries.append(entry)
        tokens_used += cost

    bundle = ContextBundle(
        bundle_id=new_id(),
        scope=scope,
        entries=entries,
        budget=budget,
        expertise_signal=estimate_expertise(scope.user_id, working_set),
    )
    working_set.record(bundle)                                       # feeds next incremental diff
    return bundle
```

## Security Considerations

Every candidate must pass a per-field Trust Boundary check *before* scoring, not after: a Semantic
Object belonging to a different customer engagement, a different organization, or a different
privacy tier than the requesting Intent's Trust Boundary is excluded from the Candidate Context
Pool entirely, never merely down-ranked (see
[15 — Security Architecture](15-security-architecture.md) and
[16 — Privacy Architecture](16-privacy-architecture.md)). This is the mechanism that prevents, for
example, one customer's confidential repository discussion from leaking into another customer's
"continue working on the API" bundle purely because both are recent. Calendar and communications
connectors are consent-gated per source, and a revoked consent immediately removes that signal
source from future assemblies (it does not retroactively scrub already-issued bundles — that is a
propagation/invalidation concern, see [07](07-context-propagation.md)). The `explain()` API exists
specifically so a user can audit *why* a sensitive object was pulled into context, per
[18 — Explainability & Trust](18-explainability-and-trust.md).

## Failure Modes

- **Over-inclusion**: pulling in more than is relevant, degrading both latency/cost and reasoning
  quality, and risking privacy exposure across scopes that scoring alone (without a hard boundary
  filter) would not catch.
- **Under-inclusion / misresolution**: "the API" resolves to the wrong repository, causing an Agent
  to work against the wrong branch or codebase silently.
- **Staleness**: a cached working-set entry (e.g., a deadline) is reused after the underlying
  Semantic Object changed, producing a bundle that is internally consistent but factually wrong.
- **Thrashing**: two entities are near-tied in relevance and the resolver flips between them across
  turns, producing an unstable context that confuses downstream Agents.

## Recovery Mechanisms

Entity resolution below the disambiguation floor never guesses silently — it escalates to
[05 — Intent Engine](05-intent-engine.md)'s ask/infer policy, which asks the user directly when the
cost of a wrong binding is high. Staleness is bounded by generation counters stamped on every
`ContextEntry` (`staleness.generation`); an Agent or the Bundle Assembler comparing a stamped
generation against the Knowledge Graph's current generation for that object can detect drift and
request a refresh rather than trusting silently — the propagation-time version of this check is
specified in [07 — Context Propagation](07-context-propagation.md). Thrashing is dampened with
hysteresis: once an entity is included in the working set, it requires a materially higher-scoring
competitor (not just a marginal win) to be displaced within the same session.

## Performance Analysis

Ranking must run in the tens-of-milliseconds range to stay inside the overall Intent-to-response
budget in [36 — Performance Benchmarks](36-performance-benchmarks.md), which is achievable because
scoring runs over a maintained working set (typically tens to low hundreds of candidates) rather
than the full Knowledge Graph, using cached embeddings and an incrementally updated index rather
than recomputing similarity from scratch. Default budget is a core bundle around 4K tokens with
`full`/`summary` entries, plus unbounded `reference` entries that cost only a pointer; an Agent that
needs deep context pays the expansion cost (`ContextEngine.expand`) only for what it actually reads,
keeping the common case cheap. Bundle reuse across a single Workspace session (incremental diffing
rather than full reassembly per turn) is the primary cost control at conversation scale.

## Trade-offs

- **Recall vs. precision vs. latency**: a wider Candidate Context Pool improves recall (fewer
  missed-context failures like the wrong repository) at the cost of ranking latency and privacy
  surface area; Hyperion biases toward precision and a tight working set, accepting that rare misses
  are corrected via the same reconciliation path used in
  [05 — Intent Engine](05-intent-engine.md#algorithms).
- **Uniform vs. per-Agent budgets**: a single global budget is simpler to reason about and audit;
  per-Agent-type budgets (a Coding Agent probably needs more repository context than a Scheduling
  Agent) are more efficient. Hyperion uses a global default with declared per-Capability overrides.
- **Caching vs. freshness**: reusing a working set across turns is far cheaper than reassembling
  from scratch, but every cached entry is a staleness risk; the generation-counter mechanism is the
  chosen middle ground over either full recomputation or unchecked caching.

## Testing Strategy

Entity-resolution accuracy is measured against a labeled corpus of ambiguous references (multiple
repositories/people/projects sharing a common short name) with a target top-1 accuracy and a
required escalation rate for genuinely ambiguous cases (escalating is a pass, silently guessing
wrong is a failure even if the guess is sometimes right). Scope-leak tests construct synthetic
multi-customer, multi-organization environments and assert zero cross-boundary inclusion under
randomized session interleaving. Latency tests hold ranking time to budget as the Knowledge Graph
scales into the millions of Semantic Objects, verifying the working-set/index approach stays
sub-linear in total graph size. Adaptive Complexity estimates are regression-tested against scripted
user sessions of known expertise level (a scripted "novice" session must never trigger `expert`-tier
UI surfaces per [13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md), and vice versa within a bounded
number of turns).

---
*Next: [07 — Context Propagation](07-context-propagation.md).*
