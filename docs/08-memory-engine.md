# Memory Engine

## 1. Purpose

The Memory Engine is the L4 [Cognition Layer](02-core-architecture.md#1-layered-system-view)
subsystem that gives Hyperion continuity across time. It stores, consolidates, decays, and —
critically — exposes for full user inspection and control everything Hyperion remembers about a
person's preferences, projects, writing and coding style, work habits, frequently used contacts,
favorite tools, ongoing goals, and past decisions. It is the subsystem that makes "continue
yesterday's work" and "you usually format code this way" true statements rather than hopeful ones.

Memory is distinct from the [Knowledge Graph](09-knowledge-graph.md): the Knowledge Graph is the
durable store of *what exists* (documents, people, projects, and their relationships); the Memory
Engine decides *what Hyperion should recall about the user, when, and with what confidence*, and
uses the Knowledge Graph as the backing store for its durable tiers (§3–4). Memory is also
distinct from [06 — Context Engine](06-context-engine.md): the Context Engine assembles a
[Context Bundle](02-core-architecture.md#context-bundle) for a specific Intent by *querying*
memory; it is a consumer, not the store.

## 2. Motivation

[01 — Vision & Philosophy](01-vision-and-philosophy.md) §7 requires Hyperion to resolve
ambiguity — "Continue yesterday's work," "Find the presentation John showed me" — using memory
rather than asking the user to restate context. §6 (Adaptive Complexity) requires the system to
continuously read a user's demonstrated experience level from accumulated behavior, not a mode
toggle. §9 (Human Control) and the brief's own instruction — "Nothing is hidden" — require every
one of these accumulated facts to be visible, editable, and deletable by the user, with no
distinction between "memory the user can see" and "memory the system quietly keeps." A memory
system that forgets everything on a fixed timer is unhelpful — it would forget an explicit
"always remember I'm allergic to peanuts" after thirty days. A memory system that forgets nothing
is both a privacy liability and a signal-to-noise problem, since every stale preference outranks
the current one. Hyperion needs a model that decays what naturally becomes irrelevant, retains
what remains true or was explicitly asked to be kept, and — unlike both extremes — never does
either invisibly.

## 3. Architecture

Memory is organized into five tiers: four distinguished by *content type* (what kind of thing is
remembered) and one, Long-Term Memory, distinguished by *role* — the durable archive the other
tiers consolidate into. This is a deliberate departure from a strict content-type taxonomy, and is
called out explicitly here rather than left implicit: cognitive-science literature usually treats
"long-term memory" as the superset containing episodic, semantic, and procedural memory, not a
fifth sibling. Hyperion's Long-Term Memory tier is that superset's *storage substrate* — the
durable, indexed, backed-up tier that Episodic, Semantic, and Procedural records graduate into
once they survive consolidation (§5), plus explicitly pinned records and compressed "gist"
summaries. Modeling it as its own tier gives it its own retention semantics, its own index, and
its own place in the transparency API, which the brief's "first-class API" requirement demands.

```
                         ┌───────────────────────────────────────┐
                         │  Memory Transparency & Control API     │
                         │  view · edit · delete · export ·       │
                         │  pin/unpin · explain (§6)               │
                         └────────────────────┬────────────────────┘
                                              │  every tier, always
        ┌───────────────┬───────────────┬─────┴───────────┬───────────────┐
        ▼               ▼               ▼                 ▼               ▼
 ┌─────────────┐ ┌─────────────┐ ┌─────────────┐   ┌──────────────┐ ┌─────────────┐
 │  WORKING    │ │  EPISODIC   │ │  SEMANTIC   │   │  PROCEDURAL  │ │  LONG-TERM  │
 │  MEMORY     │ │  MEMORY     │ │  MEMORY     │   │  MEMORY      │ │  MEMORY     │
 │ RAM, live,  │ │ event log:  │ │ fact base:  │   │ pattern/style│ │ durable     │
 │ session-    │ │ "what       │ │ "what is    │   │ store: "how  │ │ archive:    │
 │ scoped      │ │ happened,   │ │ true"       │   │ things are   │ │ consolidated│
 │             │ │ when"       │ │             │   │ usually done"│ │ + pinned    │
 └──────┬──────┘ └──────┬──────┘ └──────┬──────┘   └──────┬───────┘ └──────┬──────┘
        │ session close   │      consolidation cycle (§5)                  │
        │ distill (§5.1)  │◄──── cluster · extract · score ───────────────►│
        └────────────────►│           promote (score ≥ θ, or pinned)       │
                          └─────────────────────────────────────────────────►
                                    demote to archive (score < θ, reversible, never hard-deleted)

  Consumers: [06 Context Engine] context bundle assembly, adaptive complexity signal
             [05 Intent Engine]  goal continuity across sessions
             [09 Knowledge Graph] episodic/semantic records surface as graph nodes
             [11 Agent Runtime]  procedural style/pattern retrieval during generation
```

Working Memory lives in the process hosting the active session/Intent and is never written to
the durable stores except as a crash-recovery snapshot (see
[33 — Rollback & Recovery](33-rollback-recovery.md)). Episodic, Semantic, Procedural, and
Long-Term Memory are all persisted as [Semantic Objects](02-core-architecture.md#semantic-object)
in the [Knowledge Graph](09-knowledge-graph.md) and backed physically by the
[Storage Engine](28-storage-engine.md); this is what lets a memory record participate in graph
traversal — "the fact that I prefer dark mode" can be linked to the Settings Capability and to the
episodes that produced it.

## 4. Data Structures

The concrete on-disk schema for every table below is defined in
[29 — Database Schema](29-database-schema.md); this section defines the logical model. Every
persisted memory record shares a common envelope:

```
MemoryRecord {
  id:                ObjectID                 // same identity space as Semantic Objects
  tier:              enum{EPISODIC, SEMANTIC, PROCEDURAL, LONG_TERM}
  content:           TierPayload               // tier-specific, see below
  embedding:         vector<f32>[d]            // shared embedding space with 09-knowledge-graph.md
  created_at:        timestamp
  last_accessed_at:  timestamp
  access_count:      uint
  importance:        float [0,1]               // explicit flag OR inferred salience
  decay_score:       float [0,1]               // computed per §5.2, ignored if pinned
  pinned:            bool                      // true => permanently exempt from decay
  provenance:        [EpisodeRef]              // which interactions produced/reinforced this
  supersedes / superseded_by: ObjectID?        // version chain (Semantic Object version history)
}
```

Tier-specific payloads:

- **Working Memory** (not a `MemoryRecord`; RAM-resident only):
  `{session_id, intent_id, turns: [Turn], scratch: {...}, token_budget_remaining}` — a bounded
  ring buffer evicted oldest-first within budget, discarded at session close after distillation
  (§5.1).
- **Episodic**: `{intent_id, summary_text, entities: [ObjectID], span: {start, end}, outcome}` —
  one record per completed Intent or bounded interaction, e.g. "2026-01-14, helped user prepare
  for Acme interview, referenced résumé v3, marked completed."
- **Semantic**: `{subject, predicate, object, confidence}` fact triples, e.g.
  `(user, prefers, dark_mode)`, `(user, works_at, "Critical Software")`,
  `(user, contact_frequent, "Maria — design lead")`.
- **Procedural**: `{trigger, action_sequence: [CapabilityInvocation], confidence, style_vector?}`,
  e.g. a learned macro ("after editing a doc, user exports to PDF and emails the same two
  people") or a style vector used by [11 — Agent Runtime](11-agent-runtime.md) for generation.
- **Long-Term**: wraps a consolidated copy of any of the above plus
  `consolidation_history: [ObjectID]` — the durable, indexed, backed-up terminus of the pipeline.

## 5. Algorithms

### 5.1 Working → Episodic distillation

At session/Intent close, the Working Memory buffer is summarized by a local model
([22 — Local AI Runtime](22-local-ai-runtime.md)) into an Episode: key entities, outcome, and an
embedding of the summary. The raw turn-by-turn buffer is discarded; only the distilled Episode
persists. This bounds storage growth at the hottest tier.

### 5.2 Decay-weighted consolidation (not TTL)

Every persisted record's retrievability is governed by a weighted score, recomputed on each
consolidation pass:

```
score(r, t) = w_r · R(r, t)  +  w_f · F(r)  +  w_i · I(r)

R(r, t) = exp(-Δt / τ_tier)          Δt = t - last_accessed_at
                                      τ_tier: weeks (episodic) · months (semantic/procedural)
F(r)    = log(1 + access_count) / log(1 + access_count_norm)
I(r)    = max(explicit_importance_flag, model_estimated_salience)

if r.pinned: score := 1.0   // unconditional; recency term never applies
```

This is deliberately not a TTL: a record accessed frequently, marked important, or explicitly
pinned resists decay regardless of age — satisfying the requirement that Hyperion must never
silently forget something the user explicitly asked it to remember. An explicit "remember that
I'm allergic to peanuts" is written directly to Semantic Memory *and* Long-Term Memory with
`pinned = true`, `importance = 1.0`, bypassing the scoring pipeline entirely (§7,
`remember_explicit`).

### 5.3 Multi-stage decay funnel

`score` demotes retrieval rank before it ever removes anything:

1. **Active** (Working) — full fidelity, in RAM.
2. **Recent** (Episodic, hot index) — full fidelity, high retrieval rank.
3. **Consolidated** (Semantic/Procedural/Long-Term) — generalized, graph+vector indexed.
4. **Dormant** (`score < θ_archive`) — moved to cold storage in
   [28 — Storage Engine](28-storage-engine.md), still fully visible and exportable via the
   Transparency API (§6), just deprioritized in default recall.
5. **Purged** — reached only by explicit user deletion (`memory.erase`) or a user-configured
   retention policy (see [16 — Privacy Architecture](16-privacy-architecture.md), e.g. "auto-delete
   browsing episodes after 30 days" — opt-in, never a system default).

Automatic decay changes *prominence*, never *existence*. This is the load-bearing design choice
behind "Nothing is hidden": the system may stop surfacing a stale fact by default, but the user
can always ask to see everything, including dormant records, and nothing is deleted without
either an explicit action or a policy the user turned on themselves.

### 5.4 Extraction (Episodic → Semantic/Procedural)

The consolidation job (§7) clusters recent unconsolidated episodes by shared entities and
embedding similarity. A fact repeated across ≥ N episodes (default N=3) is promoted to a Semantic
record with `confidence = 1 − (1/2)^count`. A repeated action sequence is promoted to a Procedural
pattern the same way. This frequency gate prevents a single one-off event from being mis-learned
as a standing preference.

## 6. Interfaces / APIs

The transparency and control surface is first-class, not administrative tooling bolted on
afterward — it is exposed the same way any other Capability is, capability-secured per
[02 — Core Architecture](02-core-architecture.md#5-capability-security-as-the-unifying-security-model),
and consumed both by [13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md) (a generated "Memory
Inspector" Workspace answering "show me everything you remember about me") and directly via
[26 — APIs](26-apis.md).

```
memory.remember(fact, pin=true)      -> MemoryRecord     // explicit user "remember that..."
memory.query(filter)                 -> [MemoryRecord]   // view: tier, time range, text/embedding, pinned-only
memory.recall(query_embedding, k)    -> [MemoryRecord]   // ranked retrieval used internally by 06
memory.explain(record_id)            -> ProvenanceChain  // episodes that produced/reinforced it, decay_score, confidence
memory.edit(record_id, patch)        -> MemoryRecord     // user corrects a fact directly
memory.pin(record_id) / unpin(...)   -> MemoryRecord     // exempt from / reinstate decay
memory.erase(record_id | filter, cascade: bool = true) -> ErasureReceipt  // 16-privacy-architecture.md;
                                                                            // immediate SoftDelete
                                                                            // (default grace period);
                                                                            // cascades to dependent
                                                                            // facts unless cascade=false;
                                                                            // always audited
memory.export(filter, format)        -> Blob             // full portable export (JSON); see 16-privacy-architecture.md
```

This `memory.erase` is the Memory Engine's own entry point for the single erasure operation whose
full parameter surface (`mode: SoftDelete | CryptoShred`, multi-device propagation) is owned by
[16 — Privacy Architecture](16-privacy-architecture.md#6-interfaces--apis); calling it here is
equivalent to calling 16's `memory.erase(selector, mode=SoftDelete)` with 16's default grace
period. [26 — APIs](26-apis.md#interfaces--apis)'s `MemoryEraseRequest` is the external HTTP-shaped
contract for the same operation, returning an `ErasureReceipt` either way. There is exactly one
erasure operation across all three documents, not three.

`memory.erase` and `memory.export` require no capability beyond the user's own identity — a user
is never gated from seeing or removing their own memory by a plugin's permission model. Third-party
[Capabilities](24-plugin-framework.md) that wish to *read* memory must be granted a scoped
capability token per tier and per sensitivity level (§8); none may write to Semantic or Long-Term
Memory directly, only through `memory.remember`, which always attributes provenance to the
requesting principal.

[06 — Context Engine](06-context-engine.md) calls `memory.recall` when assembling a Context
Bundle, and reads Procedural Memory's confidence-weighted patterns as the input signal for
[01 — Vision & Philosophy](01-vision-and-philosophy.md) §6 Adaptive Complexity: a user whose
Procedural Memory shows frequent direct shell/API invocation is scored as high-complexity-ready
without any explicit mode switch, and — per that section's requirement — the read is always
explainable via `memory.explain` ("why am I seeing this?").

## 7. Pseudocode

```python
def remember_explicit(user_utterance, principal):
    """Bypass decay entirely for an explicit ask to remember something."""
    fact = extract_fact(user_utterance)                 # local model, 22-local-ai-runtime.md
    record = MemoryRecord(
        tier=SEMANTIC, content=fact,
        embedding=embed(fact.as_text()),
        importance=1.0, pinned=True, decay_score=1.0,
        provenance=[EpisodeRef(current_episode_id())],
    )
    write(record)                       # visible immediately via memory.query
    mirror_to_long_term(record)         # durable copy, consolidation_history=[record.id]
    audit_log.append("remember", principal, record.id)   # 34-observability-telemetry.md
    return record

def consolidation_cycle(now):
    """Scheduled as a low-priority background job (04-scheduler.md), idle-triggered."""
    episodes = episodic_store.unconsolidated_since(last_run_checkpoint())
    clusters = cluster_by_entity_and_embedding(episodes)

    for cluster in clusters:
        if occurs_at_least(cluster, n=3):
            fact_or_pattern = extract(cluster)           # §5.4
            existing = semantic_or_procedural_store.find_matching(fact_or_pattern)
            if existing:
                merge_with_version_history(existing, fact_or_pattern)   # keeps prior version
            else:
                promote(fact_or_pattern, tier=SEMANTIC_OR_PROCEDURAL)

    for record in all_persisted_records():
        if record.pinned:
            continue                                      # never touched by decay
        record.decay_score = score(record, now)            # §5.2
        if record.decay_score >= THETA_PROMOTE and record.tier != LONG_TERM:
            promote_to_long_term(record)
        elif record.decay_score < THETA_ARCHIVE:
            move_to_cold_storage(record)                    # reversible, still queryable/exportable
        # THETA_PURGE is never checked here — purge is user- or policy-initiated only

    checkpoint(now)
    audit_log.append("consolidation_cycle", now, stats=cluster_stats(clusters))
```

## 8. Security Considerations

Memory is a persistent record of a person's life; it is treated as sensitive by default (see
[16 — Privacy Architecture](16-privacy-architecture.md)) — encrypted at rest via
[28 — Storage Engine](28-storage-engine.md), access capability-scoped per tier and per sensitivity
class (health, financial, and legal facts require step-up authentication before they are read
into a Context Bundle destined for a lower-trust [Agent](02-core-architecture.md#agent) or a
cloud Capability). The most distinctive risk is **memory poisoning via prompt injection**: content
Hyperion reads from an untrusted source (a web page, an email body, a shared document) must never
be able to silently write a durable Semantic or Procedural record. Only utterances attributable to
an authenticated human principal on a trusted input channel (the conversational shell, per
[15 — Security Architecture](15-security-architecture.md)) may call `memory.remember` directly;
content ingested from external sources may populate Working Memory and inform a single Intent's
Context Bundle, but can only reach Semantic/Long-Term Memory through the normal frequency-gated
consolidation path (§5.4), which requires the pattern to repeat across independently-sourced
episodes — a single malicious document cannot manufacture a "fact about the user." Every write,
edit, pin, and forget is attributed and audited
([34 — Observability & Telemetry](34-observability-telemetry.md)), and cross-device memory sync
crosses a [Trust Boundary](02-core-architecture.md#trust-boundary) exactly like any other
capability-secured call ([21 — Distributed Execution](21-distributed-execution.md)).

## 9. Failure Modes

- **Consolidation crash mid-cycle** leaves some clusters promoted and others not — must be
  idempotent and checkpointed (§10).
- **Importance misjudgment** archives something the user still cared about (false negative in
  `I(r)`) — mitigated by the pinning mechanism and by dormant records remaining fully
  visible/searchable, never deleted.
- **Over-eager procedural learning** from a one-off event — mitigated by the N-occurrence
  frequency gate (§5.4) and trivial correction via `memory.edit`/`memory.erase`.
- **Cross-device write conflicts** when two devices update the same fact while offline —
  resolved per the Knowledge Graph's concurrency model (see
  [09 — Knowledge Graph](09-knowledge-graph.md) §Algorithms), since all durable memory records
  are Semantic Objects.
- **Context Bundle over-inclusion** leaking a sensitive memory record to an Agent that didn't
  need it — mitigated by per-record sensitivity tags enforced at Trust Boundary crossings (§8).

## 10. Recovery Mechanisms

Every mutation (edit, forget, consolidation-driven merge) produces a prior version rather than
overwriting in place, per [02 — Core Architecture](02-core-architecture.md#4-design-invariants)
invariant #2 ("everything is undoable or versioned"); `memory.erase` creates a tombstone with a
recovery grace period before physical deletion, surfaced in
[33 — Rollback & Recovery](33-rollback-recovery.md). The consolidation job checkpoints after each
cluster so a crash resumes from the last completed cluster rather than reprocessing or
double-promoting. Because automatic decay only ever moves a record to cold storage rather than
deleting it, an overly-aggressive decay pass is always recoverable by the user through
`memory.query(tier=ALL, include_dormant=true)` or simply asking Hyperion to "look further back."
Full export (§6) doubles as an out-of-band backup.

## 11. Performance Analysis

Working Memory operations are in-process and O(1) amortized (bounded ring buffer, no disk I/O).
Episodic/Semantic recall is an ANN vector search over the shared embedding index bounded by
[06 — Context Engine](06-context-engine.md)'s context-assembly latency budget and the
conversational responsiveness targets in [36 — Performance Benchmarks](36-performance-benchmarks.md)
— target sub-100ms for typical recall queries at up to millions of records. Consolidation is a
batch job scheduled at low priority during idle windows ([04 — Scheduler](04-scheduler.md)) so it
never competes with foreground Intents; its cost is amortized and grows with unconsolidated
episode count since the last checkpoint, not total history size. Cold-tier storage is
intentionally cheap (compressible, tiered to slower media in
[28 — Storage Engine](28-storage-engine.md)) so "never truly purge automatically" does not
translate into expensive hot storage growth.

## 12. Trade-offs

Weighted decay over TTL costs tuning complexity (per-tier τ, an importance estimator) in exchange
for not discarding what matters — an explicit design bet justified by the brief's "must not
silently forget" requirement. Defaulting to *never automatically purge* (only demote) costs
unbounded-looking storage growth in exchange for satisfying "Nothing is hidden" without
qualification; the cost is bounded in practice by cheap cold-tier storage and by making auto-purge
an opt-in policy for users who want it. A fine-grained, always-on transparency API (§6) adds UI and
engineering surface area that a system optimizing purely for simplicity would omit — justified
directly by [01 — Vision & Philosophy](01-vision-and-philosophy.md) §9's non-negotiable Human
Control principle.

## 13. Testing Strategy

Unit tests validate per-tier CRUD and schema conformance. Property-based tests assert the pinning
invariant holds under fuzzing — a pinned record's `decay_score` never drops below 1.0 regardless
of simulated time or access patterns. Long-run simulation tests replay synthetic multi-month usage
traces and check extracted Semantic/Procedural facts against a labeled ground truth, and check the
decay curve's half-life against the configured τ within tolerance. Adversarial tests confirm
untrusted document content cannot reach Semantic/Long-Term Memory in fewer than the required N
independent episodes (§8). A global "no hidden writes" conformance check asserts every code path
that persists a memory record is reachable from `memory.query` — no write bypasses the
transparency API. Chaos tests kill the consolidation job mid-cycle and verify idempotent resume
with no duplication or loss. Scale tests validate the recall latency budget at multi-year usage
volumes, feeding into [37 — Scalability Roadmap](37-scalability-roadmap.md), and cross-device sync
tests validate convergence per [21 — Distributed Execution](21-distributed-execution.md).

---
*Next: [09 — Knowledge Graph](09-knowledge-graph.md).*
