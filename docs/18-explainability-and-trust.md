# Explainability & Trust

This document is the deep engineering treatment of
[01 — Vision & Philosophy §9, "Human Control Is Non-Negotiable"](01-vision-and-philosophy.md#9-human-control-is-non-negotiable):
every autonomous action Hyperion takes must be interruptible, undoable, auditable, observable,
explainable, and modifiable. It defines the concrete data structure and machinery that make
"explainable" true in practice — not a UX tagline but a queryable record attached to every
autonomous action — and the control plane that makes "interruptible" and "modifiable" true. It
does not own capability tokens, sandboxing, or the decision of *how much* autonomy an action is
granted (that risk-assessment engine belongs to
[15 — Security Architecture](15-security-architecture.md)); this document owns *what a user is
told* about an action and *how a user can stop or redirect it*, whatever its risk tier.

## 1. Purpose

Attach a durable, queryable **Explanation Record** to every action any [Agent](02-core-architecture.md#agent)
or [Capability](02-core-architecture.md#capability) takes without a human directly clicking
"go" for that specific step, capturing the triggering Intent, the reasoning chain and evidence
used, a confidence score, the alternatives considered and why they were rejected, and a pointer
into the undo mechanism in [33 — Rollback & Recovery](33-rollback-recovery.md). Provide the query
interface a user invokes to ask "why did you do that?" and resolve it back to that record. Define
how explanations compose when an action is the joint product of several Agents coordinated by
[12 — Multi-Agent Coordination](12-multi-agent-coordination.md), and define the interruption and
modification control plane that lets a user stop or redirect an Agent mid-flight.

## 2. Motivation

Autonomy without an answer to "why" is not assistance, it is a black box that happens to be
usually right — and "usually right" is not a bar an operating system that manages a user's goals,
memory, and knowledge graph can accept. [01 — Vision & Philosophy §5](01-vision-and-philosophy.md#5-universal-usability-highest-priority)
states plainly that "usability without trust is not usability." Three concrete situations force
this document to exist rather than remain a principle:

1. A user sees an unexpected change in their [Workspace](02-core-architecture.md#workspace) — a
   file moved, a meeting rescheduled, a message drafted — and has no route back to *which* Agent
   did it, *why*, and *whether it can be undone right now, mid-action*.
2. An action is the emergent result of three coordinated Agents (Research, Scheduling, Coaching in
   the worked trace of [02 — Core Architecture §3](02-core-architecture.md#3-how-a-request-flows-through-the-layers)) — a single flat log line cannot represent whose
   evidence drove the final outcome.
3. A user wants to interrupt an Agent that is visibly heading in the wrong direction, but by the
   time a "stop" click reaches the process, three more side effects have already landed —
   interruption has to be a scheduling primitive, not a hope.

## 3. Architecture

Explanation is recorded **causally, at decision time**, not reconstructed after the fact by asking
a model to rationalize what it already did — post-hoc rationalization is a known failure mode
(confabulation, see §9) that this architecture is built to avoid. The Explanation Recorder is
co-located, in-process, with the Agent making the decision, and its write is coupled to the
action's own transaction: an effect is not permitted to commit without its Explanation Record
committing alongside it.

```
        05 Intent                    11 Agent Runtime / 12 Multi-Agent Coordination
            │                                        │
            ▼                                        ▼
   ┌──────────────────┐    emits at        ┌─────────────────────────┐
   │  Decision Point    │   decision time   │  Explanation Recorder     │
   │ (Agent selects a    │──────────────────▶│  in-process, WAL-coupled  │
   │  plan / action)     │                  │  to the action's own       │
   └──────────────────┘                    │  commit (§9)               │
                                             └────────────┬────────────┘
                                                           ▼
                                             ┌─────────────────────────┐
                                             │  Explanation Store         │
                                             │  encrypted at the same      │
                                             │  privacy tier as the data   │
                                             │  it references — see 16     │
                                             └────────────┬────────────┘
                          ┌────────────────────────────────┼───────────────────────────────┐
                          ▼                                ▼                                ▼
              ┌───────────────────────┐       ┌─────────────────────────┐      ┌─────────────────────────┐
              │ "Why did you do that?" │       │ Multi-Agent Merge         │      │ 34 Observability &         │
              │  Query API (§6)         │       │ ExplanationGraph (§5)      │      │ Telemetry audit ledger      │
              │                         │       │                            │      │ (full record; access-       │
              │                         │       │                            │      │  controlled per 16, never   │
              │                         │       │                            │      │  redacted — see §8)         │
              └───────────────────────┘       └─────────────────────────┘      └─────────────────────────┘

  Control plane (independent channel, always live regardless of explanation state):
  User ──interrupt/modify──▶ 04 Scheduler preemption ──▶ 11 Agent Runtime checkpoint ──▶ 33 Rollback
```

The "Explanation Store" above is not a second database alongside
[34 — Observability & Telemetry](34-observability-telemetry.md)'s audit ledger — it *is* that
ledger's security-relevant-event path (34 §Architecture: grants, revocations, and Explanation
Records flow through a separate, never-sampled, durability-first path straight into the audit
ledger, bypassing the general telemetry redactor entirely). An `ExplanationRecord` is therefore
stored in full, exactly as produced, with no redaction step at write time; what protects a
sensitive explanation from an unauthorized reader is 16's access control on the query path
(`explain.why`/`explain.trace`, §6), not a lossy transform applied before storage — an explanation
about private data is still private data, and inherits that data's access rules rather than having
them stripped out (§8).

## 4. Data Structures

```rust
struct ExplanationRecord {
    id: ExplanationId,
    action_id: ActionId,                 // the concrete effect this record explains
    triggering_intent_id: IntentId,      // 05 Intent Engine
    agent_id: AgentId,                   // 11 Agent Runtime
    capability_id: CapabilityId,         // 02 §Capability
    created_at: Timestamp,
    reasoning_chain: [ReasoningStep],
    evidence: [EvidenceRef],
    confidence: ConfidenceScore,
    alternatives: [Alternative],
    undo_ref: Option<RecoveryPointId>,   // 33 — Rollback & Recovery
    trust_boundary_span: [TrustBoundaryId], // 02 §Trust Boundary crossings this action made
    privacy_class: SensitivityClass,     // 16 — inherited from the data referenced
    parent_records: [ExplanationId],     // multi-agent composition, §5
    child_records: [ExplanationId],
    control_state: ControlState,
}

struct ReasoningStep {
    step_index: u32,
    description: String,                 // rendered at the user's Adaptive Complexity level, 01 §6
    tool_or_capability_used: Option<CapabilityId>,
    inputs_ref: [ObjectRef],             // 09 Semantic Objects consulted
    output_ref: Option<ObjectRef>,
}

struct EvidenceRef {
    object_id: ObjectId,
    excerpt_or_summary: String,
    weight: f32,                         // contribution to the final decision, 0..1
}

struct ConfidenceScore {
    value: f32,                          // calibrated, 0..1
    method: SelfConsistency | Verifier | Ensemble | Heuristic,
    calibration_set_ref: Option<String>,  // for auditing calibration drift, see §11
}

struct Alternative {
    description: String,
    score: f32,
    rejection_reason: String,
}

enum ControlState { Proposed, Executing, Completed, Interrupted, Modified, RolledBack }

// Multi-agent composition: a DAG, not a flat list.
struct ExplanationGraph {
    root: ExplanationId,                 // the coordination-level action the user asked about
    nodes: Map<ExplanationId, ExplanationRecord>,
    edges: [(ExplanationId, ExplanationId)], // parent contributed-to child
}
```

## 5. Algorithms

**Explain-then-commit recording.** Recording is part of the action's transaction, not a side
effect of it: an Agent assembles the `ExplanationRecord` as it reasons (each `ReasoningStep`
appended as the reasoning happens, each `EvidenceRef` attached as the evidence is actually
consulted), and the effect is only permitted to commit once the record is durably written. This
ordering is what distinguishes a genuine reasoning trace from a rationalization generated after
the outcome is already known.

**Confidence calibration.** `ConfidenceScore.value` is not the Agent's raw self-reported certainty;
it is produced by one of the declared `method`s — self-consistency across repeated sampling,
a lightweight verifier pass, ensemble agreement across
[23 — Multi-Model Orchestration](23-multi-model-orchestration.md)'s candidate models, or a
heuristic for deterministic Capabilities — and is periodically checked against actual outcomes
(§11) to catch calibration drift.

**Multi-agent merge.** When [12 — Multi-Agent Coordination](12-multi-agent-coordination.md)
produces a result from several Agents, each contributing Agent writes its own
`ExplanationRecord` and links it as a `parent_records` entry of the coordination-level record. A
user query resolves to the root record first — a single headline sentence — and expands, on
request, by walking the `ExplanationGraph` depth-first, so disagreement between Agents (a
Scheduling Agent overruling a Research Agent's suggested time, say) is visible rather than
smoothed into one narrative that hides it.

**Query resolution.** `explain.query(action_id)` looks up the record, renders `reasoning_chain` and
`alternatives` at the user's current Adaptive Complexity level (01 §6 — a beginner sees the
headline and undo button; a developer can request the full chain including raw evidence weights),
and recursively resolves `parent_records` only if the caller asks for `depth=full`.

**Interruption and modification.** A user-issued interrupt is a signal, not a request: it is
delivered through the same preemption channel [04 — Scheduler](04-scheduler.md) uses for any other
priority preemption, targeting the Agent's next declared **safe checkpoint** in
[11 — Agent Runtime](11-agent-runtime.md)'s lifecycle (Agents must declare checkpoints between
side-effecting steps precisely so an interrupt has somewhere safe to land). A `Modify` signal
carries a patch to the Intent or a parameter and is applied at the same checkpoint, letting the
Agent continue with adjusted goals rather than restarting from zero.

## 6. Interfaces / APIs

```
explain.query(action_id, depth: headline | full) -> ExplanationView
explain.why(natural_language_ref) -> ExplanationView   // resolves via 06 Context Engine to the
                                                        // most contextually relevant recent action_id
explain.trace(intent_id) -> ExplanationGraph            // every action taken under one Intent

control.interrupt(action_id, mode: Pause | Abort)
control.modify(action_id, patch: IntentPatch)
control.resume(action_id)

events.on_autonomous_action(ExplanationRecord)  -> emitted to 34 — Observability & Telemetry
events.on_control_signal(action_id, signal, delivered_at, applied_at)
```

`explain.why` is the natural-language entry point a user actually uses in the conversational shell
("why did you do that?", "why is this here?") — it never requires the user to know an `action_id`;
resolution against recent Context is delegated to
[06 — Context Engine](06-context-engine.md).

## 7. Pseudocode

```python
def perform_autonomous_action(agent, intent, plan, risk: RiskAssessment):
    # `risk` is computed upstream by 15's Risk Assessment Engine before this function is ever
    # invoked (15 §3: the engine intercepts a pending action before the Agent Runtime acts on
    # it). Its `recovery_point_ref` is already None unless `intervention_level >=
    # REQUIRE_BACKUP_FIRST` (15 §7) — this function trusts and records that decision rather
    # than re-deriving it or unconditionally taking a synchronous checkpoint of its own, which
    # would defeat the entire point of 15/33's risk-tiered recovery-point cost model.
    record = ExplanationRecord(
        id=new_id(), action_id=plan.action_id, triggering_intent_id=intent.id,
        agent_id=agent.id, capability_id=plan.capability_id,
        reasoning_chain=[], evidence=[], alternatives=[], control_state=PROPOSED,
    )

    for step in plan.reasoning_steps():
        record.reasoning_chain.append(step.render())
        record.evidence.extend(step.evidence_consulted())
        if scheduler.pending_interrupt(plan.action_id):
            return handle_interrupt(agent, plan, record)   # land at this safe checkpoint

    record.confidence = calibrate_confidence(plan, record.evidence)
    record.alternatives = plan.rejected_alternatives()
    record.control_state = EXECUTING
    record.undo_ref = risk.recovery_point_ref   # None for routine actions, covered instead by
                                                  # 33's Automatic periodic batched checkpoint

    explanation_store.commit(record)            # durable BEFORE the effect executes — always,
                                                  # regardless of whether undo_ref is set
    effect = agent.capability.invoke(plan)
    record.control_state = COMPLETED
    explanation_store.update(record)

    telemetry.emit(events.on_autonomous_action(record))  # full record, not redacted — see §3/§8
    return effect


def resolve_why(action_id, depth="headline"):
    record = explanation_store.get(action_id)
    if record is None:
        return best_effort_reconstruction(action_id)        # §9 — replay from Event System, 31
    view = render_at_complexity_level(record, current_user_level())
    if depth == "full":
        for parent_id in record.parent_records:
            view.children.append(resolve_why(parent_id, depth="full"))
    return view
```

## 8. Security Considerations

Explanation Records are tamper-evident via the same mechanism
[34 — Observability & Telemetry](34-observability-telemetry.md) uses for its whole audit ledger: a
single global, monotonic hash chain (`entry_hash = H(prev_hash || canonical(payload) || seq)`),
not a chain scoped per Agent — a per-Agent view is a filtered *read* over that one global chain
(indexed by `agent_id`), not a second chaining scheme, so a compromised Agent cannot quietly
rewrite its own history without also breaking the chain for every entry after it, Agent-scoped or
not. Access to an
`explain.query` result is gated by the same capability grant that gated the underlying data — a
user (or a Plugin acting on their behalf) cannot use the explanation channel as a side door to read
data they were never granted access to; this is the concrete tie to
[16 — Privacy Architecture](16-privacy-architecture.md) that the brief calls out: an explanation
about private data is itself private data, and inherits that data's `SensitivityClass` and
encryption tier rather than being stored in the clear "for convenience." Control-plane signals
(`interrupt`, `modify`) are authenticated exactly like any other Trust-Boundary-crossing call — a
malicious Capability must not be able to forge a user-issued interrupt to abort a competitor's
Agent, nor suppress a genuine interrupt meant for itself.

## 9. Failure Modes

- **Crash before the record commits.** Because the record commits before the effect executes
  (§7), a crash mid-decision leaves no orphaned, unexplained effect — there is either a complete
  record with a completed effect, or neither.
- **Confabulation.** A model's free-text self-report of "why" can diverge from what it actually
  computed. Mitigated by grounding `reasoning_chain` entries in actual tool calls and
  `EvidenceRef`s pulled from the real execution trace, not a separate free-text summarization pass
  invited to invent detail.
- **Unbounded multi-agent graph depth.** Deep coordination chains could produce an
  `ExplanationGraph` too large to render usefully; resolution collapses beyond a configurable depth
  into a "+N more agent steps, expand to see" summary rather than failing the query.
- **Interrupt race.** A `Pause` signal can arrive after the Agent has already passed its last
  checkpoint and committed; in that case the interrupt is honored as an immediate follow-up
  `Modify`/undo request against the just-completed action rather than silently dropped.
- **Explanation store unavailable.** `explain.query` degrades to `best_effort_reconstruction`,
  replaying [31 — Event System](31-event-system.md) logs to approximate the record and flagging the
  result as reconstructed, never presenting a best-effort guess as an authoritative record.

## 10. Recovery Mechanisms

The rollback checkpoint referenced by `undo_ref` is opened before the effect executes and is the
concrete mechanism [33 — Rollback & Recovery](33-rollback-recovery.md) uses to make "undoable"
true even mid-action: an `Abort` control signal both halts further execution and drives the
in-flight effect back to that checkpoint. Interrupt delivery escalates if unacknowledged — a soft
pause request, then a hard preemption via [04 — Scheduler](04-scheduler.md), then a sandbox freeze
via [15 — Security Architecture](15-security-architecture.md) — so a non-responsive Agent is never
uninterruptible in practice. Confidence miscalibration discovered after the fact (an action marked
high-confidence that a user then reversed) feeds back into the calibration set referenced in
`ConfidenceScore.calibration_set_ref`, tightening future scores for that Agent/Capability pair.

## 11. Performance Analysis

Recording overhead targets under 5 ms per `ReasoningStep` append, a small fraction of the model
inference or Capability latency it accompanies. Explanation Store growth follows data growth and
is bounded by the same lifecycle and erasure rules as the objects it references (§8, §16) — an
erased Semantic Object's explanation evidence is erased with it, not orphaned. Query latency
targets under 300 ms for a cached headline explanation and scales with `ExplanationGraph` depth for
`depth=full` queries. Interrupt delivery targets under 100 ms to the nearest safe checkpoint, bounded
by the checkpoint granularity Agents declare in [11 — Agent Runtime](11-agent-runtime.md) — an
Agent that declares coarse-grained checkpoints trades interrupt responsiveness for recording
overhead, and this specification requires checkpoint granularity to scale inversely with an
action's risk tier as assessed by [15](15-security-architecture.md): high-risk actions get frequent
checkpoints and full-chain explanations; routine, low-risk, high-frequency actions get lightweight
single-line explanations to keep the aggregate system responsive.

## 12. Trade-offs

Full causal fidelity (every tool call, every evidence weight) is the most trustworthy record but
the most verbose; the layered rendering in §6 resolves this by always giving a one-sentence
headline with drill-down on demand, matching the Adaptive Complexity philosophy of
[01 — Vision & Philosophy §6](01-vision-and-philosophy.md#6-adaptive-complexity) rather than forcing
every user to read a full reasoning trace. Real-time interrupt guarantees are bounded, not
instantaneous, because an in-flight write cannot be aborted mid-byte without a transactional undo —
Hyperion chooses "interrupt within one checkpoint interval, always successfully" over "instant
interrupt, sometimes leaves a mess." Per-agent granular explanations versus one merged narrative is
resolved by keeping both: a single headline answer for the common case, with the
`ExplanationGraph` available for a user who wants to see which Agent actually decided what — this
costs additional storage and query complexity in exchange for not hiding inter-agent disagreement.
Finally, privacy and auditability pull in opposite directions; this specification keeps full detail
in the encrypted store (auditable, in principle, forever) rather than truncating detail from the
audit log, and instead gates *access* to that detail (§8) rather than gating its *existence*.

## 13. Testing Strategy

Explanation faithfulness is evaluated against a golden set that compares a rendered explanation to
the actual causal trace captured by the recorder, flagging any divergence as a confabulation
regression. Fault injection tests kill an Agent mid-decision and assert that no effect survives
without a matching completed `ExplanationRecord`, and that an incomplete record triggers the
rollback path in §10. Interrupt-under-load benchmarks verify the 100 ms checkpoint-delivery target
holds as concurrent Agent count scales, per the workloads modeled in
[36 — Performance Benchmarks](36-performance-benchmarks.md). Multi-agent merge correctness tests
assert that a coordination session's `ExplanationGraph` topology matches the actual coordination
graph produced by [12 — Multi-Agent Coordination](12-multi-agent-coordination.md), not a
simplification of it. Privacy regression tests reuse the suite from
[16 — Privacy Architecture §13](16-privacy-architecture.md), asserting `explain.query` never
returns detail the caller lacks a capability grant for. Calibration drift is tracked over time with
a rolling Brier score per Agent/Capability, feeding an alert if an Agent's stated confidence
systematically diverges from observed outcomes.

---
*Next: [19 — Networking Stack](19-networking-stack.md).*
