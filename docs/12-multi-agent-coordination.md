# Multi-Agent Coordination

This document defines the [L5 Coordination Layer](02-core-architecture.md#1-layered-system-view)
subsystem that turns a decomposed [Intent Graph](02-core-architecture.md#2-shared-vocabulary) into
a team of cooperating [Agent Runtime](11-agent-runtime.md) instances, and coordinates them toward
one shared goal. Where [11 — Agent Runtime](11-agent-runtime.md) defines how a single Agent is
instantiated, sandboxed, and resourced, this document defines how *many* Agents — each an
independent Trust Boundary — allocate work, share state safely, resolve disagreement, and report
combined progress without ever letting the user lose control of the outcome, per
[01 — Vision & Philosophy §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable).

## 1. Purpose

Multi-Agent Coordination is the subsystem that answers one question: given an Intent Graph too
large for one Agent to execute alone, who does what, in what order, sharing what state, and who
tells the user how it's going? It owns: task allocation from Intent Graph nodes to Agent
instances (§5.1); a shared, versioned plan data structure all participating Agents read and write
(§4.1); a coordination protocol carried over [30 — IPC Framework](30-ipc-framework.md); conflict
resolution when two Agents disagree or collide (§5.2); progress aggregation for
[13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md) and
[18 — Explainability & Trust](18-explainability-and-trust.md); and failure containment so that one
Agent's failure degrades gracefully instead of silently corrupting the goal. It does not itself
reason about the goal's content — that remains the job of the individual Agents and the
[05 — Intent Engine](05-intent-engine.md) that produced the Intent Graph in the first place.

## 2. Motivation

The product brief's example is deliberately ambitious: **"Launch my product"** should
automatically decompose into coordinated Agents responsible for Engineering, Website, Marketing,
Legal, Documentation, Customer Support, Analytics, Finance, Branding, and Deployment, with the
user simply watching progress and providing guidance when desired. This is a qualitatively
different problem from the single-Agent case in [11](11-agent-runtime.md): ten Agents will read
and write overlapping Semantic Objects (a shared product name, a shared launch date, a shared
brand asset), will sometimes propose contradictory plans (Marketing wants to launch Friday,
Engineering's Intent Graph shows QA won't finish until Monday), and one of them failing (Legal
blocked on an external filing) must not silently leave "Launch my product" reporting as on-track
when it is not. Without a dedicated coordination layer, this either requires each Agent to
hard-code awareness of every other Agent (an N² integration problem that breaks the moment a new
Agent type is added) or requires the user to manually stitch results together — which fails the
Golden Rule in [01 §2](01-vision-and-philosophy.md#2-the-golden-rule): the OS, not the user, should
do the coordinating.

## 3. Architecture

```
┌───────────────────────── L4 Cognition Layer ─────────────────────────────┐
│     Intent Engine (05) — Intent Graph for "Launch my product"            │
└───────────────────────────────┬───────────────────────────────────────────┘
                                 │ decompose() / notify on Intent change
┌────────────────────────────────▼──────────────────────────────────────────┐
│                  MULTI-AGENT COORDINATION   (L5 Coordination Layer)       │
│                                                                             │
│   ┌────────────────┐        ┌──────────────────────┐                      │
│   │  Decomposer      │──────▶│   Task Allocator       │                     │
│   │ (Planning Agent, │       │  capability match +    │                     │
│   │  see 11 §4)      │       │  load balance  (§5.1)  │                     │
│   └────────────────┘        └───────────┬───────────┘                     │
│                                          │ assign(task, agent)              │
│      ┌─────────────┬──────────┬─────────┼─────────┬──────────┬─────────┐  │
│      ▼             ▼          ▼         ▼         ▼          ▼         ▼  │
│  [Engineering] [Website] [Marketing] [Legal] [Docs] [Support] ... [Deploy] │
│    Agent        Agent      Agent      Agent  Agent   Agent        Agent   │
│      │             │          │         │         │          │         │  │
│      └─────────────┴────┬─────┴─────────┴─────────┴──────────┘         │  │
│                          │ proposeWrite / claimTask / reportStatus         │
│                 ┌─────────▼──────────┐                                    │
│                 │  Shared Plan /      │◀── conflict escalation (§5.2)      │
│                 │  Blackboard (§4.1)  │     resolved by Coordinator Agent   │
│                 │  versioned          │     or escalated to user           │
│                 └─────────┬──────────┘                                    │
│                          │ weighted rollup (§5.3)                          │
└──────────────────────────┼──────────────────────────────────────────────┘
                           │
        ┌───────────────────┼─────────────────────┐
        ▼                   ▼                       ▼
Dynamic UI Runtime(13)  Explainability &      IPC Framework (30) /
  progress workspace     Trust Log (18)       Context Propagation (07)
                                              (shared Context Bundle scoping)
```

All point-to-point inter-Agent traffic — task proposals, claims, blackboard writes, conflict
flags — is carried as messages over [30 — IPC Framework](30-ipc-framework.md); there is no
side-channel between Agent sandboxes. Progress and escalation traffic (§7's `subscribeProgress`/
`escalate`) is broadcast, many-to-many, with a subscriber set the publisher doesn't know in
advance (typically both [13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md) and
[18 — Explainability & Trust](18-explainability-and-trust.md) at once) — that shape is
[31 — Event System](31-event-system.md)'s territory, not IPC's, per
[30's own point-to-point/broadcast distinction](30-ipc-framework.md#architecture); it is carried
there, not over IPC. Agents sharing this goal are given overlapping *slices* of a
common [Context Bundle](02-core-architecture.md#2-shared-vocabulary), propagated per
[07 — Context Propagation](07-context-propagation.md) — each Agent still only sees the slice
relevant to its own task, per least privilege (§7).

## 4. Data Structures

### 4.1 Shared Plan (Blackboard)

```
type GoalId = UUID   // alias of SharedPlan.session_id; the identifier 33-rollback-recovery.md's
                       // UndoScope::Goal and Trigger::PreGoalFork carry — a decomposed Intent
                       // Graph's shared goal and its coordination session are the same scope

SharedPlan {
  session_id     : UUID
  root_intent_id : IntentRef             // the top-level Intent Graph node, e.g. "Launch my product"
  version        : uint64                 // monotonic, incremented on every accepted write
  nodes          : [TaskNode]
  participants   : [AgentInstanceRef]     // see 11-agent-runtime.md §5.2
  conflicts      : [ConflictRecord]
  status_summary : ProgressSummary        // §5.3
}

TaskNode {
  task_id                : UUID
  sub_intent_id          : IntentRef       // maps 1:1 to an Intent Graph node, see 05-intent-engine.md
  description            : string
  required_capabilities  : [CapabilityRef] // used for allocation matching, §5.1
  required_trust_tier    : TrustTier       // TrustTier defined in 11-agent-runtime.md §5.1,
                                            // 1:1 with 15-security-architecture.md's
                                            // provenance_tier; hard eligibility gate, §5.1
                                            // below — not a scoring input
  assigned_agent         : AgentInstanceRef?
  status                 : enum { unassigned, claimed, in_progress, blocked, done, failed }
  dependencies           : [task_id]
  outputs                : [SemanticObjectRef]
  base_version           : uint64          // plan version this node was last read at
}
```

### 4.2 Coordination Message Envelope

```
CoordMessage {
  message_id   : UUID
  session_id   : UUID
  from         : AgentInstanceRef
  to           : AgentInstanceRef | BROADCAST
  type         : enum { PROPOSE_TASK, CLAIM_TASK, UPDATE_PLAN, REPORT_STATUS,
                         FLAG_CONFLICT, ESCALATE }
  plan_version : uint64          // optimistic-concurrency check, §5.2
  payload      : bytes
}
```

### 4.3 Conflict Record

```
ConflictRecord {
  conflict_id  : UUID
  object_ref   : SemanticObjectID | task_id
  claimants    : [AgentInstanceRef]
  kind         : enum { concurrent_write, contradictory_subplan, resource_contention }
  resolution   : enum { pending, auto_merged, coordinator_resolved, user_resolved }
}
```

## 5. Algorithms

### 5.1 Task Allocation — Capability Matching + Load Balancing

The Decomposer (typically a Planning Agent per [11 §4](11-agent-runtime.md#4-built-in-agent-specializations))
turns the Intent Graph into an initial set of `TaskNode`s, one per sub-intent. Allocation is then a
scored bipartite assignment, not an arbitrary or first-come assignment:

1. For each unassigned `TaskNode`, compute the candidate set: Agent specializations whose manifest
   `baseline_capabilities ∪ requestable_capabilities` (see
   [11 §5.1](11-agent-runtime.md#5-data-structures)) is a superset of `required_capabilities`,
   **and** whose manifest `trust_tier` meets or exceeds `required_trust_tier` — trust is a hard
   eligibility filter applied at candidate-set construction, exactly like capability matching, not
   a scoring input; an insufficiently-trusted Agent is never a candidate, so it can never
   outrank a properly-trusted one regardless of capability fit.
2. Score each remaining candidate by: capability fit (exact match scores higher than a broader
   superset), current load (active `TaskNode` count and quota headroom from
   [11 §6.2](11-agent-runtime.md#62-resource-quota--backpressure)), and historical performance on
   this `sub_intent` category (from [08 — Memory Engine](08-memory-engine.md)).
3. Assign the highest-scoring candidate; ties break toward the least-loaded instance. If no
   candidate exists, spawn a new instance of the best-fit specialization via
   [11 — Agent Runtime](11-agent-runtime.md#7-interfaces--apis) rather than leaving the node
   unassigned.
4. Dependencies (`TaskNode.dependencies`) gate `claimed → in_progress`; an Agent may claim a
   downstream task early to reserve it, but cannot start executing until its dependencies reach
   `done`.

This is deliberately an auction-like greedy matching rather than a global optimum solver: Intent
Graphs change shape as Agents make discoveries (Engineering may spawn a new sub-intent Marketing
didn't anticipate), so allocation must be incremental and cheap to re-run, not a one-shot optimal
assignment computed once.

### 5.2 Conflict Resolution

Two Agents can collide in two distinct ways, and the resolution path differs:

- **Concurrent write to the same Semantic Object** (e.g., Branding and Website both propose a
  product name change). Writes carry the `plan_version` (or object version) they were read at —
  optimistic concurrency control. If the version has since advanced, the write is rejected and
  re-diffed: mergeable diffs (non-overlapping fields) are auto-merged and re-applied; overlapping,
  non-mergeable diffs raise a `ConflictRecord` of kind `concurrent_write`.
- **Contradictory sub-plans** (e.g., Marketing's plan assumes Friday launch, Engineering's Intent
  Graph shows Monday completion). Detected when two `TaskNode`s' stated assumptions about a shared
  fact (a date, a budget figure, a scope boundary) diverge. This raises a `contradictory_subplan`
  conflict.

Both kinds route to the same escalation ladder: (1) attempt automatic resolution if the conflict
type has a known-safe merge rule; (2) if not, escalate to a **Coordinator Agent** — a Planning
Agent instance holding elevated visibility over the whole `SharedPlan` — which arbitrates using
the Intent Graph's own stated priorities and dependency order; (3) if the Coordinator cannot
resolve within a bounded number of rounds, or the conflict touches a high-consequence Semantic
Object (legal filings, payments, anything flagged by [16 — Privacy Architecture](16-privacy-architecture.md)),
escalate directly to the user via [13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md), per
[01 §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable). No conflict is ever
resolved by simply letting the last write win silently.

### 5.3 Progress Aggregation

Each `TaskNode` carries a weight (estimated effort, or an explicit user-assigned priority).
Overall progress is a weighted rollup:

```
progress(plan) = Σ_i ( weight_i × completion_fraction(node_i) ) / Σ_i weight_i
```

This produces both a single top-level percentage for
[13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md)'s progress surface, and a per-branch breakdown
(Engineering 80%, Legal 20% — blocked) that
[18 — Explainability & Trust](18-explainability-and-trust.md) turns into a narrative explanation
rather than a bare number, so the user always sees *why* the aggregate is what it is, not just
what it is.

### 5.4 Failure Containment

Each Agent's health is monitored the same way [11 — Agent Runtime §6.2](11-agent-runtime.md#62-resource-quota--backpressure)
monitors any instance — heartbeats, circuit breakers, quota breaches. Coordination adds one rule on
top: an Agent's uncommitted writes are held in a **quarantine buffer** and never merged into the
`SharedPlan` until the write is accepted (§5.2); if the Agent fails before that, the quarantine
buffer is simply discarded — the shared goal state never observes a half-finished write. The
failed `TaskNode` is marked `failed`, its dependents are marked `blocked`, and reallocation (§5.1)
is retried up to a bounded limit before escalating to the user as a named, specific blocker ("Legal
is stuck — the filing requires a decision only you can make") rather than a silent stall.

## 6. Interfaces / APIs

```
Coordination.createSession(root_intent_id) -> session_id
Coordination.decompose(session_id) -> [TaskNode]                      // delegates to 05-intent-engine.md
Coordination.allocate(session_id) -> [assignment]                     // §5.1
Coordination.claimTask(agent_instance_id, task_id) -> ack | rejected
Coordination.proposeWrite(agent_instance_id, object_ref, base_version, diff) -> accepted | ConflictRecord
Coordination.reportStatus(agent_instance_id, task_id, status, evidence) -> ack
Coordination.subscribeProgress(session_id) -> ProgressStream           // carried over
                                                                          // 31-event-system.md
                                                                          // (many-to-many,
                                                                          // subscriber-unknown-
                                                                          // to-publisher — feeds
                                                                          // 13-dynamic-ui-runtime.md
                                                                          // and 18 simultaneously)
Coordination.escalate(session_id, conflict_id | reason) -> UserPrompt  // same broadcast path;
                                                                          // feeds 13 and 18
```

Every call above is itself Capability-secured and crosses a Trust Boundary exactly like any
[11 — Agent Runtime](11-agent-runtime.md#7-interfaces--apis) call; the coordination session holds
no ambient authority over participant Agents beyond what each Agent's own manifest already grants.

## 7. Pseudocode

```python
def allocate_task(plan: SharedPlan, node: TaskNode, registry: AgentRegistry) -> AgentInstanceRef:
    # Trust is a hard eligibility gate, applied before scoring — never a tiebreaker. An
    # Agent whose manifest trust_tier doesn't meet required_trust_tier is not a candidate,
    # full stop, no matter how good its capability fit.
    candidates = [
        a for a in registry.active_instances(plan.session_id)
        if node.required_capabilities <= a.manifest.granted_capabilities()
        and a.manifest.trust_tier <= node.required_trust_tier   # TrustTier is ordered
                                                                   # System(0) < Verified(1) <
                                                                   # Community(2), same
                                                                   # direction as 15's
                                                                   # provenance_tier, so a
                                                                   # *lower* value is *more*
                                                                   # trusted and "<=" means
                                                                   # "at least as trusted as
                                                                   # required"
    ]
    if not candidates:
        best_spec = registry.best_fit_specialization(node.required_capabilities,
                                                        min_trust_tier=node.required_trust_tier)
        candidates = [agent_runtime.spawn(best_spec, node.sub_intent_id, plan.context_bundle_ref)]

    def score(agent):
        fit    = capability_fit(agent.manifest, node.required_capabilities)
        load   = 1.0 / (1 + agent.active_task_count())
        perf   = memory_engine.historical_performance(agent.specialization, node.category)
        return (fit, load, perf)

    best = max(candidates, key=score)
    node.assigned_agent = best.instance_id
    node.status = "claimed"
    plan.version += 1
    ipc.send(CoordMessage(type="UPDATE_PLAN", to=best.instance_id,
                           plan_version=plan.version, payload=node))
    return best.instance_id


def propose_write(plan: SharedPlan, agent: AgentInstanceRef, object_ref, base_version, diff):
    current = plan.object_version(object_ref)
    if base_version == current:
        plan.apply(object_ref, diff)
        plan.version += 1
        broadcast_update(plan)
        return "accepted"

    merged = try_auto_merge(diff, plan.pending_diffs(object_ref))
    if merged is not None:
        plan.apply(object_ref, merged)
        plan.version += 1
        return "accepted"

    conflict = ConflictRecord(object_ref=object_ref, claimants=plan.claimants(object_ref),
                               kind="concurrent_write", resolution="pending")
    plan.conflicts.append(conflict)
    resolution = coordinator_agent.arbitrate(conflict, plan)          # §5.2 escalation ladder
    if resolution == "unresolved":
        ui.escalate_to_user(plan.session_id, conflict)                # 13-dynamic-ui-runtime.md
    return conflict
```

## 8. Worked Example: "Launch My Product"

1. The user says "Launch my product." [05 — Intent Engine](05-intent-engine.md) produces an Intent
   Graph with a root Intent and ten candidate sub-intents matching the brief: Engineering,
   Website, Marketing, Legal, Documentation, Customer Support, Analytics, Finance, Branding,
   Deployment.
2. Coordination's Decomposer (§5.1) turns each sub-intent into a `TaskNode`, computing
   `required_capabilities` from its content (e.g., Legal needs `contract.review`,
   `regulatory.check`).
3. The Task Allocator matches and spawns ten Agent instances (per
   [11 — Agent Runtime](11-agent-runtime.md)), each bound to its `TaskNode`'s sub-intent and a
   scoped slice of the shared Context Bundle — Branding sees the product's visual assets and name
   candidates; Legal sees the entity's jurisdiction and existing contracts; neither sees the
   other's full scope.
4. Agents proceed in parallel. The Branding Agent proposes a product name; the Legal Agent
   independently flags a trademark conflict on a similar name candidate mid-review. This raises a
   `contradictory_subplan` conflict (§5.2): Branding's plan assumes name A, Legal's finding rules
   name A out. The Coordinator Agent arbitrates using the Intent Graph priority (legal risk
   outranks branding preference by policy) and reassigns Branding's `TaskNode` back to
   `in_progress` with the constraint attached.
5. The Deployment Agent's `TaskNode` depends on Engineering's `done` status; it remains `blocked`
   and correctly reports so in the progress rollup (§5.3), rather than appearing stalled with no
   explanation.
6. The Finance Agent's task fails outright (an external payment-processor API is unreachable).
   Failure containment (§5.4) marks the node `failed`, quarantines its uncommitted writes, retries
   allocation once, and — on second failure — escalates: the user sees a specific, named blocker
   in the [13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md) Workspace ("Finance setup is stuck —
   the payment processor didn't respond. Retry, or switch providers?") rather than a stalled
   progress bar.
7. The user, watching the aggregated Workspace, answers that one prompt; every other branch
   continues untouched. When all `TaskNode`s reach `done`, the root Intent transitions to
   `completed` per its lifecycle in [05 — Intent Engine](05-intent-engine.md), and the full
   ten-Agent trace is available as one coherent explanation via
   [18 — Explainability & Trust](18-explainability-and-trust.md).

## 9. Security Considerations

Coordination does not create a new authority surface — it composes existing ones. Each Agent in a
team still only holds the Capability grants its own manifest earns under
[11 — Agent Runtime §6.1](11-agent-runtime.md#61-capability-grant-resolution); the Shared Plan
(§4.1) is not a side channel for one Agent to act through another's grants. Context Bundle slices
are scoped per Agent per [07 — Context Propagation](07-context-propagation.md) — the Finance
Agent never receives Legal's privileged contract text unless a `TaskNode` explicitly requires it,
which is itself an auditable grant. Blackboard writes are checked against the target Semantic
Object's ACL exactly as a single-Agent write would be under
[15 — Security Architecture](15-security-architecture.md); coordination membership in a session
confers no implicit write permission. The Coordinator Agent's arbitration authority (§5.2) is
itself just another Agent manifest grant (`coordination.arbitrate`), auditable and revocable, not
a hidden superuser role.

## 10. Failure Modes

- **Silent goal corruption** — an Agent writes plausible-looking but wrong data into the Shared
  Plan. Prevented by the quarantine-until-accepted rule (§5.4) and by versioned optimistic
  concurrency (§5.2) making every accepted write attributable and diffable.
- **Deadlock between dependent Agents** — two `TaskNode`s each waiting on the other's output due
  to a decomposition error. Detected via cycle-checking on `dependencies` at allocation time
  (§5.1) and re-flagged to the Decomposer if a cycle is found post hoc.
- **Cascading failure** — one failed Agent's blocked dependents blocking their own dependents.
  Contained by surfacing the *root* blocker (§8 step 6) rather than reporting every downstream
  symptom as an independent failure.
- **Contradictory sub-plans looping** — Coordinator arbitration flip-flopping between two Agents'
  conflicting assumptions. Bounded by the retry ceiling in §5.4/§5.2; exceeding it forces user
  escalation rather than infinite arbitration.
- **Status noise** — ten Agents each reporting fine-grained status overwhelms the user. Contained
  by the weighted rollup (§5.3) presenting one aggregate with drill-down, not ten raw feeds.

## 11. Recovery Mechanisms

The `SharedPlan`'s version history is itself a sequence of
[33 — Rollback & Recovery](33-rollback-recovery.md) recovery points: any accepted write can be
reverted to a prior plan version without affecting Agents whose tasks didn't touch the reverted
object. A failed Agent's `TaskNode` is requeued through the allocation algorithm (§5.1) exactly as
if it were newly created, so recovery reuses the same code path as initial assignment rather than
a special-cased repair routine. Unresolved conflicts and repeated failures always terminate in a
user-facing escalation (§5.2, §5.4) rather than an automatic decision on the user's behalf for
anything consequential, satisfying
[01 — Vision & Philosophy §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable). The
full session — Shared Plan, every Agent Execution Record (per
[11 §5.4](11-agent-runtime.md#54-agent-execution-record-audit)), and every conflict resolution — is
replayable for post-hoc audit via [18 — Explainability & Trust](18-explainability-and-trust.md).

## 12. Performance Analysis

Allocation (§5.1) is O(n·m) per re-run for n unassigned `TaskNode`s and m active Agent instances —
cheap enough to re-run incrementally as the Intent Graph evolves rather than requiring a global
solve. Conflict detection (§5.2) is O(1) per write via version comparison; auto-merge attempts are
bounded by a fixed per-object diff window to avoid pathological merge costs. Message throughput
across [30 — IPC Framework](30-ipc-framework.md) scales linearly with team size for the common
case (status reports, task claims) but conflict escalation traffic is intentionally rare and
bounded by the retry ceiling. For very large teams (tens of Agents), the single Shared Plan can
become a write-contention hotspot; the plan is therefore partitioned by object-affinity so
unrelated branches (e.g., Documentation and Deployment) rarely contend on the same version
counter, consistent with the scaling posture of
[37 — Scalability Roadmap](37-scalability-roadmap.md).

## 13. Trade-offs

- **A centralized Shared Plan/blackboard** is simpler to reason about and audit than a fully
  decentralized gossip protocol between Agents, at the cost of being a potential bottleneck and
  single point of failure — mitigated by object-affinity partitioning (§12) rather than abandoning
  centralization, since auditability is a harder requirement here than raw throughput per
  [18 — Explainability & Trust](18-explainability-and-trust.md).
- **Greedy incremental allocation** (§5.1) sacrifices global optimality for the ability to
  re-allocate cheaply as the Intent Graph changes shape mid-execution — appropriate because
  real goals like "Launch my product" are discovered incrementally, not known in full up front.
- **Coordinator Agent arbitration** is faster than always escalating to the user, but introduces a
  layer whose own misjudgment is possible; bounding its retry budget and mandating user escalation
  beyond that bound (§5.2) is the chosen balance between autonomy and
  [01 §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable)'s Human Control principle.

## 14. Testing Strategy

Simulation tests exercise the allocation algorithm (§5.1) against synthetic Intent Graphs of
varying width and depth, verifying assignment quality and re-allocation cost after mid-run
mutation. Chaos tests kill individual Agents at each `TaskNode` status transition, asserting the
quarantine-and-reallocate path (§5.4) never leaves the `SharedPlan` in a partially-written state.
Adversarial tests construct deliberately contradictory sub-plans to verify the escalation ladder
(§5.2) terminates in bounded rounds rather than looping. Load tests scale team size into the tens
of Agents to validate the partitioned Shared Plan under §12's contention model. A dedicated
end-to-end regression test encodes the full "Launch my product" trace from §8 as a fixture,
verifying the aggregate progress rollup, the Legal/Branding conflict resolution, and the
Finance-failure escalation reproduce deterministically, integrated with
[35 — Testing Strategy](35-testing-strategy.md).

---
*Next: [13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md) renders the progress and guidance
surfaces this document's aggregation (§5.3) and escalation (§5.2, §5.4) paths feed into.*
