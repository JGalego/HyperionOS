# Context Propagation

## Purpose

[02 — Core Architecture](02-core-architecture.md#context-bundle) defines the Context Bundle and
states that "the wire format and propagation rules are defined in 07 — Context Propagation." This
document is that specification: how a Context Bundle assembled by
[06 — Context Engine](06-context-engine.md) is serialized, scoped, and carried across the
boundaries where it stops being a single in-process data structure and becomes something that must
survive a hop — Agent-to-Agent handoff within
[12 — Multi-Agent Coordination](12-multi-agent-coordination.md), device-to-device continuity in
[21 — Distributed Execution](21-distributed-execution.md), and crossings of a Trust Boundary
enforced by [15 — Security Architecture](15-security-architecture.md) and
[16 — Privacy Architecture](16-privacy-architecture.md). It also defines when a propagated bundle
must be treated as stale rather than trusted, and how this point-to-point mechanism differs from
the broadcast [31 — Event System](31-event-system.md).

## Motivation

A Context Bundle that never left the process that assembled it would need no wire format at all.
Three facts about Hyperion make that untrue:

1. **Intents outlive a single Agent.** [05 — Intent Engine](05-intent-engine.md) hands a whole
   Intent Graph to [12 — Multi-Agent Coordination](12-multi-agent-coordination.md), which
   distributes sub-intents across multiple Agents, potentially in separate sandboxes or containers
   (see [03 — Kernel Architecture](03-kernel-architecture.md)). Each handoff is a boundary crossing.
2. **Users move across devices mid-task**, per
   [01 — Vision & Philosophy §7](01-vision-and-philosophy.md#7-human-language-first)'s "Continue
   yesterday's work" — which must work whether "yesterday" was on the same laptop or a different
   phone, per [21 — Distributed Execution](21-distributed-execution.md).
3. **Not every recipient is equally trusted.** A Context Bundle assembled inside a fully-trusted
   personal Workspace cannot be handed whole to a third-party Capability sandboxed by
   [15 — Security Architecture](15-security-architecture.md) — some fields must be redacted or
   scoped down before the crossing, per [16 — Privacy Architecture](16-privacy-architecture.md) and
   Design Invariant 1 in [02 §4](02-core-architecture.md#4-design-invariants) ("no silent
   authority").

Without an explicit propagation layer, each of these three crossings would invent its own ad hoc
serialization and its own ad hoc redaction logic — exactly the kind of accidental complexity
Hyperion's single capability-security model (
[02 §5](02-core-architecture.md#5-capability-security-as-the-unifying-security-model)) is designed
to avoid.

## Architecture

```
   In-process Context Bundle (06 — Context Engine)
                     │
                     ▼
        ┌─────────────────────────────────────────────┐
        │              CONTEXT PROPAGATION              │
        │                                                │
        │   ┌─────────────────────────────────────┐     │
        │   │  export(bundle, target_boundary)      │     │
        │   │   1. classify target trust level      │     │
        │   │   2. scope/redact per policy (15/16)   │     │
        │   │   3. choose by-ref vs by-value entries │     │
        │   │   4. stamp staleness + provenance       │     │
        │   │   5. sign envelope                      │     │
        │   └───────────────────┬─────────────────────┘     │
        │                       ▼                            │
        │            CONTEXT ENVELOPE (wire format)          │
        └───────────────────────┬────────────────────────────┘
                                 │
        ┌────────────────────────┼─────────────────────────────┐
        ▼                        ▼                              ▼
┌───────────────┐      ┌──────────────────┐          ┌────────────────────────┐
│ 30 — IPC       │      │ 21 — Distributed  │          │ 15 — Trust Boundary /   │
│ Framework      │      │ Execution         │          │ 16 — Privacy Arch.      │
│ (local agent   │      │ (device sync:     │          │ (redaction gate before  │
│ handoff, 12)   │      │ laptop → phone)   │          │ third-party Capability) │
└───────┬────────┘      └────────┬──────────┘          └────────────┬────────────┘
        └────────────────────────┼──────────────────────────────────┘
                                  ▼
                     ┌─────────────────────────────────┐
                     │      import(envelope)             │
                     │  1. verify signature/integrity     │
                     │  2. check staleness/generation     │
                     │  3. rehydrate references (lazy)     │
                     │  4. reconstruct local ContextBundle │
                     └─────────────────────────────────┘
                                  │
                                  ▼
                     Receiving Agent / device / sandbox
```

Contrast with [31 — Event System](31-event-system.md), shown for scale: Event System messages are
**broadcast, fire-and-forget notifications** ("build succeeded," "intent.status_changed") consumed
by zero or more subscribers with no expectation of complete task state and no per-recipient
redaction beyond topic-level ACLs. Context Propagation is **unicast, task-scoped, and stateful** —
exactly one recipient receives exactly the (possibly redacted) slice of state it needs to continue
a specific Intent, and the sender must know it was delivered.

## Data Structures

```
ContextEnvelope {
  envelope_id: EnvelopeID
  schema_version: string                       // forward/backward-compat contract
  bundle_scope: { intent_id, agent_invocation_id? }
  entries: [EnvelopeEntry]
  provenance: {
    originating_device_id, originating_session_id,
    agent_chain: [AgentRef],                    // handoff history for audit, 18
    capability_token: CapabilityToken,           // proves authority to propagate, 02 §5
  }
  scope_applied: RedactionPolicyRef              // which policy produced this envelope
  staleness: {
    per_entry_generation: Map<SemanticObjectRef, GenerationVector>,
    freshness_horizon: duration,                 // beyond this, must re-validate before trusting
    captured_at: timestamp,
  }
  integrity: { signature, hash_alg }
}

EnvelopeEntry {
  category: string                               // mirrors ContextEntry.category, 06
  representation: by_reference | by_value | redacted_placeholder
  ref: SemanticObjectRef | null
  inline_value: bytes | null                     // present only for by_value
  redaction_reason: string | null                 // present only for redacted_placeholder
}

RedactionPolicy {
  target_trust_level: TrustLevel
  field_rules: [{ category, action: pass | summarize | redact }]
  default_action: redact                         // fail-closed: unknown category => redacted
}
```

The critical structural decision is `EnvelopeEntry.representation`: an entry crossing a local,
same-Trust-Boundary hop (Agent-to-Agent within one user's session) is typically `by_reference` —
cheap, and the receiving Agent can call back into
[09 — Knowledge Graph](09-knowledge-graph.md) for the full object. An entry crossing an actual
Trust Boundary is never `by_reference`, because a reference is itself a grant of query authority;
it is either `by_value` (a redacted, self-contained snapshot) or `redacted_placeholder` (structure
preserved, content withheld, so the receiving Agent knows *that* something was omitted rather than
silently reasoning over an incomplete world).

## Algorithms

**1. Representation selection.** `by_reference` is chosen when the transport is local IPC
([30 — IPC Framework](30-ipc-framework.md)) and sender/recipient share a Trust Boundary. Everything
else defaults to `by_value` with redaction applied first, because references are only meaningful —
and only safe — inside a shared authority domain.

**2. Redaction/scoping.** `export()` looks up the `RedactionPolicy` for the target's trust level
(derived from [15 — Security Architecture](15-security-architecture.md)'s classification of the
recipient — another first-party Agent, a sandboxed third-party Capability, a remote device) and
computes, per entry, `pass` (include as-is or summarized), or `redact` (replace with a typed
placeholder that names the category and reason, e.g. `redacted: financial-detail`, per
[16 — Privacy Architecture](16-privacy-architecture.md)). The default action for any category not
explicitly listed in the policy is `redact` — this is fail-closed by construction, satisfying
Design Invariant 1 ([02 §4](02-core-architecture.md#4-design-invariants)): an unrecognized field
never crosses a Trust Boundary by accident.

**3. Cross-device continuity.** [21 — Distributed Execution](21-distributed-execution.md) may have
two devices with independently-evolved local context (the user edited on the phone while the laptop
was offline). Propagation between devices uses a generation-vector-per-object merge: entries that
differ only in which device last touched them merge on last-writer-wins by generation; entries with
conflicting divergent edits to the *same* field are flagged for the specific Agent or the user to
resolve rather than silently picked, since silent resolution here risks losing user intent exactly
as an unresolved editor merge conflict would.

**4. Staleness and invalidation.** Every entry's `staleness.per_entry_generation` is compared, at
import time, against the current generation of that Semantic Object in
[09 — Knowledge Graph](09-knowledge-graph.md). Three outcomes: (a) generation matches — trust as
current; (b) generation is behind but within `freshness_horizon` — trust provisionally, but flag
for background revalidation; (c) generation is behind *and* beyond the horizon, or the underlying
object was force-mutated (the canonical example: "continue yesterday's work" after the repository
was force-pushed) — the entry is marked `stale`, downgraded to `redacted_placeholder`-like
treatment (structurally present, content untrustworthy), and the importing Agent must re-fetch or
explicitly ask the user before acting on anything downstream of it. This is a hard rule, not a
heuristic: no Agent may execute an irreversible action against a `stale`-flagged entry.

**5. Integrity.** Every envelope is signed at export using the exporting principal's key; `import()`
verifies the signature before anything else runs, and rejects (rather than degrades) on failure —
this is the one step in the pipeline that fails closed on the whole envelope, because a tampered or
replayed envelope cannot be partially trusted.

## Interfaces / APIs

```
ContextPropagation.export(bundle, target_boundary) -> ContextEnvelope
ContextPropagation.import(envelope) -> ContextBundle
ContextPropagation.checkStaleness(envelope_or_bundle) -> FreshnessReport
ContextPropagation.merge(bundleA, bundleB) -> ContextBundle | ConflictReport
ContextPropagation.revalidate(entry_ref) -> EnvelopeEntry     // background refresh path
```

`export`/`import` are invoked by [12 — Multi-Agent Coordination](12-multi-agent-coordination.md) on
every Agent handoff, by [21 — Distributed Execution](21-distributed-execution.md) on every
cross-device sync, and by the [15 — Security Architecture](15-security-architecture.md) sandbox
boundary whenever a Capability outside the current Trust Boundary is invoked. Transport itself is
delegated: local hops ride [30 — IPC Framework](30-ipc-framework.md); cross-device hops ride
[21 — Distributed Execution](21-distributed-execution.md)'s sync channel. Context Propagation owns
only the envelope contract and the redaction/staleness logic, not the bytes-on-the-wire transport.

## Pseudocode

```python
def export(bundle, target_boundary):
    trust_level = SecurityArchitecture.classify(target_boundary)          # 15
    policy = RedactionPolicy.for_trust_level(trust_level)                  # 16

    entries = []
    for e in bundle.entries:
        rule = policy.rule_for(e.category, default="redact")
        if rule == "redact":
            entries.append(EnvelopeEntry(category=e.category,
                                          representation="redacted_placeholder",
                                          redaction_reason=f"policy:{trust_level}"))
            continue
        rep = "by_reference" if same_trust_boundary(target_boundary) else "by_value"
        entries.append(materialize_entry(e, representation=rep, summarize=(rule == "summarize")))

    envelope = ContextEnvelope(
        envelope_id=new_id(),
        schema_version=CURRENT_SCHEMA,
        bundle_scope=bundle.scope,
        entries=entries,
        provenance=build_provenance(bundle, target_boundary),
        scope_applied=policy.id,
        staleness=stamp_generations(entries),
        integrity=sign(entries),
    )
    audit_log.record("context.exported", envelope.envelope_id, target_boundary)  # 18
    return envelope


def import_envelope(envelope):
    if not verify_signature(envelope):
        raise IntegrityError("envelope failed verification")               # fail closed, no partial trust

    report = check_staleness(envelope)
    for entry, status in report.per_entry_status.items():
        if status == "stale_beyond_horizon":
            entry.representation = "redacted_placeholder"
            entry.redaction_reason = "stale"
        elif status == "stale_within_horizon":
            schedule_background_revalidation(entry)                        # provisional trust

    bundle = ContextBundle(
        bundle_id=derive_local_id(envelope),
        scope=envelope.bundle_scope,
        entries=[rehydrate(e) for e in envelope.entries],                  # lazy for by_reference
        assembled_at=now(),
    )
    audit_log.record("context.imported", envelope.envelope_id)
    return bundle


def merge(bundle_a, bundle_b):
    merged, conflicts = [], []
    for ref in union_of_refs(bundle_a, bundle_b):
        ea, eb = entry_for(bundle_a, ref), entry_for(bundle_b, ref)
        if ea is None or eb is None:
            merged.append(ea or eb)
        elif ea.staleness.generation == eb.staleness.generation:
            merged.append(ea)
        elif touches_same_field_divergently(ea, eb):
            conflicts.append((ea, eb))                                     # surfaced, not auto-picked
        else:
            merged.append(latest_generation(ea, eb))
    if conflicts:
        return ConflictReport(conflicts)
    return ContextBundle(entries=merged)
```

## Security Considerations

The redaction gate is the enforcement point for Design Invariant 1 (no silent authority) at the
context layer specifically: a Context Bundle is itself a form of authority (it tells a recipient
what it may reason about, and often implicitly what it may act on), so exporting one across a Trust
Boundary is treated with the same rigor as granting a capability token — indeed `provenance.
capability_token` ties every exported envelope to the specific grant that authorized the crossing,
auditable per [18 — Explainability & Trust](18-explainability-and-trust.md). Signing prevents
tampering in transit, particularly relevant for the [21 — Distributed Execution](21-distributed-execution.md)
path, which may traverse an untrusted network. Fail-closed default redaction means a newly-added
context category that no one has classified yet is invisible to any recipient outside the local
Trust Boundary until a human explicitly extends the policy — the safe failure direction given
[02 §4](02-core-architecture.md#4-design-invariants)'s degrade-never-fail-closed principle applies
to the user's *goal*, not to unreviewed data exposure.

## Failure Modes

- **Silently trusted stale context**: the canonical case — "continue yesterday's work" after the
  underlying repository was force-pushed, deleted, or renamed. Without the staleness check in
  Algorithm §4, an Agent would act against a codebase state that no longer exists.
- **Partial envelope loss**: a flaky cross-device link truncates or drops an envelope mid-transfer.
- **Replay**: a captured, previously-valid envelope is resubmitted later to re-trigger stale
  authority.
- **Over-aggressive redaction**: a policy misconfiguration redacts a field an Agent actually needs,
  degrading task quality without a security benefit.
- **Merge conflict mishandling**: two divergent device states silently resolved the wrong way,
  quietly discarding a user's edit.

## Recovery Mechanisms

Staleness beyond the freshness horizon degrades an entry to a placeholder rather than failing the
whole envelope import — the receiving Agent can still act on the *unaffected* parts of the bundle
and either re-fetch the stale entry or ask the user, consistent with Design Invariant 5 (degrade,
never fail closed on the user's goal). Envelope IDs are single-use; `import()` maintains a
short-window replay cache and rejects a repeat `envelope_id`, closed by
[15 — Security Architecture](15-security-architecture.md)'s broader anti-replay mechanisms. Partial
transfer loss is handled by chunked, resumable transport at the
[21 — Distributed Execution](21-distributed-execution.md) layer with an envelope-level checksum, so
a truncated envelope is detected and re-requested rather than imported incomplete. Merge conflicts
that cannot be resolved automatically are surfaced through the same clarifying-question path used
by [05 — Intent Engine](05-intent-engine.md#algorithms) rather than resolved by an arbitrary
tiebreak.

## Performance Analysis

Local, same-Trust-Boundary handoffs (the common case: Agent-to-Agent inside one Intent Graph's
execution) are dominated by `by_reference` entries and are expected to add sub-millisecond overhead
on top of [30 — IPC Framework](30-ipc-framework.md)'s baseline, since no redaction computation or
snapshotting is needed when sender and recipient share a Trust Boundary and policy is trivially
"pass all." Cross-device propagation is bandwidth-bound rather than compute-bound: by-reference
entries are converted to lightweight pointers plus the minimal inline snapshot needed for offline
use, keeping typical envelopes in the low tens of kilobytes even for a rich working session; full
object bodies are fetched lazily on the receiving device only when an Agent actually dereferences
them. Trust-Boundary crossings pay the redaction-computation cost, which scales with entry count,
not entry size, and is budgeted well under the per-Capability-invocation latency target in
[36 — Performance Benchmarks](36-performance-benchmarks.md).

## Trade-offs

- **By-reference vs. by-value**: references are cheap and always current but require connectivity
  and shared trust to resolve; values are self-contained and safe to redact but heavier and prone to
  staleness. The representation-selection rule in Algorithm §1 is the chosen split, not a universal
  default in either direction.
- **Strict vs. permissive redaction**: fail-closed redaction is the safer default but can starve a
  legitimately-needed Agent of context it needs to do its job well; the mitigation is an explicit,
  auditable policy-extension path rather than loosening the default.
- **CRDT-style merge vs. last-writer-wins**: full conflict-free merge semantics are more automatic
  but harder to reason about and audit; Hyperion uses generation-based last-writer-wins for
  non-overlapping changes and surfaces genuine conflicts to a human rather than implementing general
  CRDT merge logic, trading some automation for auditability.

## Testing Strategy

A redaction test matrix crosses every context category against every trust level to assert the
correct action (`pass`/`summarize`/`redact`) and, critically, that any category absent from a
policy resolves to `redact`. Staleness tests inject synthetic backing-store mutations (including a
simulated force-push) between export and import and assert the entry is downgraded and no Agent
acts on it without revalidation. A device-hop simulation (laptop → phone → laptop, with induced
network partition) exercises `merge()` and asserts non-overlapping edits merge automatically while
overlapping edits are surfaced, never silently dropped. Adversarial tests attempt envelope replay
and signature tampering and assert hard rejection. Chaos tests on a degraded network verify partial
transfers are detected and resumed rather than imported truncated.

---
*Next: [08 — Memory Engine](08-memory-engine.md).*
