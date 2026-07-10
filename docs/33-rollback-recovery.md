# Rollback & Recovery

## Purpose

This document specifies the recovery-point and undo mechanism referenced throughout this
specification: the concrete meaning of a **recovery point**, the undo API surface (single-action,
session-level, and "undo everything this Agent did"), how far back undo can reach, how automatic
and user-triggered recovery points differ, how a crash mid-Agent-execution recovers without
corrupting shared multi-agent state, and how a staged rollout's rollback
([32 — Update System](32-update-system.md)) folds in cleanly. This is the mechanism
[15 — Security Architecture](15-security-architecture.md)'s risk-assessment engine invokes before
risky autonomous actions, that [18 — Explainability & Trust](18-explainability-and-trust.md)'s
Explanation Record points to as evidence an action was reversible, and that
[28 — Storage Engine](28-storage-engine.md)'s versioning primitives implement underneath.

## Motivation

[02 — Core Architecture §4](02-core-architecture.md#4-design-invariants) invariant 2 states the
requirement this entire document exists to satisfy: *"everything is undoable or versioned. State-
changing operations produce a recovery point before they execute."* [01 — Vision &
Philosophy §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable) sharpens this into a
user-facing promise — every autonomous action must be undoable, "reversed cleanly, not just
'sorry, that can't be undone.'" A recovery point cannot be a file backup, because Hyperion does
not organize state as files: a single Intent can simultaneously write a
[Semantic Object](02-core-architecture.md#semantic-object)'s content (blob store), its typed
relationships ([09 — Knowledge Graph](09-knowledge-graph.md), graph store), its embeddings (vector
store), and its permissions and version metadata (metadata store) — see
[29 — Database Schema](29-database-schema.md) for the four-store model. A recovery point must
therefore be a **consistent cut across all four stores simultaneously**, or "undo" would restore
content while leaving stale relationships or embeddings behind, which is arguably worse than not
undoing at all: the user would trust a state that is quietly self-contradictory. Multi-agent
work ([12 — Multi-Agent Coordination](12-multi-agent-coordination.md)) raises the same requirement
at a larger scope: several Agents can share a single goal's state, and an abandoned or crashed
goal must roll back as one atomic unit, not leave one Agent's contribution undone and another's
standing.

## Architecture

Three cooperating components implement the mechanism: the **Recovery Point Service** (a
capability inside [28 — Storage Engine](28-storage-engine.md)) that produces consistent
cross-store snapshots; the **Action/Intent Journal**, an append-only, hash-chained log that
records every state-changing operation together with the recovery point taken immediately before
it; and the **Undo Coordinator**, which resolves an undo *request* (scoped to one action, one
session, one Agent run, or the whole system) into either a direct restoration to a recovery point
or a targeted replay of inverse operations.

```
   Triggers: 15 risk engine (pre-risky-action) · 32 update system (pre-apply)
             12 multi-agent (pre-goal-fork) · user "undo" / "undo everything X did"
                                       │
                                       ▼
                        ┌───────────────────────────────┐
                        │        Undo Coordinator         │
                        │  resolves UndoScope → action set │
                        └────────────────┬─────────────────┘
                                        │
                                        ▼
                        ┌───────────────────────────────┐
                        │   Action / Intent Journal        │◄──── every state-changing op
                        │   append-only, hash-chained      │      appends one ActionRecord,
                        └────────────────┬─────────────────┘      tagged with Intent/Agent id
                                        │ each ActionRecord references
                                        ▼
                        ┌───────────────────────────────┐
                        │      Recovery Point Ledger        │
                        └────────────────┬─────────────────┘
                                        │ one atomic cut across all four stores
              ┌────────────┬─────────────┼─────────────┬────────────┐
              ▼            ▼             ▼             ▼
        ┌──────────┐ ┌──────────┐  ┌───────────┐ ┌────────────┐
        │  Blob     │ │  Graph    │  │  Vector    │ │  Metadata   │
        │  Store    │ │  Store    │  │  Store     │ │  Store      │
        │(28-*.md)  │ │(28/09-*)  │  │(28-*.md)   │ │(29-*.md)    │
        └──────────┘ └──────────┘  └───────────┘ └────────────┘
```

### What a recovery point concretely is

A `RecoveryPoint` is **not** a copy of data; it is a durable, timestamped *reference* — one
snapshot identifier per store, taken at (as close as physically possible to) the same logical
instant, plus the causal predecessor recovery point it extends. Each store implements its own
cheap, native snapshotting primitive appropriate to its structure: the blob store uses
content-addressed, copy-on-write references (a snapshot is just "the set of blob hashes live at
this instant," which costs nothing to take because blobs are already immutable and
content-addressed); the graph store uses a multi-version concurrency control (MVCC) timestamp
cut; the vector store snapshots its index generation pointer; the metadata store (object
permissions, version history) uses the same MVCC timestamp as the graph store, since the two are
transactionally linked. The Recovery Point Service's job is *coordination*, not storage: it asks
all four stores to report a snapshot reference as of one logical clock value, and only commits the
`RecoveryPoint` record once all four have confirmed — never partially.

### Undo scopes

Undo operates at three granularities, all resolved by the same Undo Coordinator against the same
journal:

1. **Single-action undo** — reverse exactly one `ActionRecord` (e.g., "undo that file rename").
   Resolved via the action's declared inverse operation where one exists (cheap, surgical), or by
   restoring only the specific objects it touched to their pre-action state pulled from the
   bracketing recovery point (still surgical, but requires a per-object diff rather than a
   symbolic inverse).
2. **Session-level undo** — reverse a contiguous run of actions within one user session, in
   reverse chronological order, stopping either at a target action or at the session boundary.
   This is the everyday "keep pressing undo" behavior.
3. **Agent-run undo ("undo everything this Agent did")** — resolved against
   [11 — Agent Runtime](11-agent-runtime.md)'s run identifier: every `ActionRecord` tagged with
   that Agent-run ID is identified, and the Undo Coordinator restores the union of touched objects
   to the recovery point taken when the Agent's run began — a single semantic operation from the
   user's perspective, even though it may reverse dozens of underlying writes across several
   Capabilities.

## Data Structures

```rust
struct RecoveryPoint {
    id: RecoveryPointId,
    logical_time: HybridLogicalClock,     // orders recovery points across stores/nodes
    causal_predecessor: Option<RecoveryPointId>,
    blob_snapshot: BlobSnapshotRef,        // content-hash set reference; ~free to take
    graph_snapshot: GraphSnapshotRef,      // MVCC cut
    vector_snapshot: VectorSnapshotRef,    // index generation pointer
    metadata_snapshot: MetadataSnapshotRef,// MVCC cut, same timestamp as graph
    trigger: Trigger,
    retention_class: RetentionClass,
    pinned: bool,
}

enum Trigger {
    Automatic,                 // periodic / low-risk batched checkpoint
    UserRequested,
    PreRiskyAction,            // 15-security-architecture.md risk engine
    PreUpdate(UpdateSubject),  // 32-update-system.md
    PreAgentRun { agent_run_id: AgentRunId },  // 11 / 12
    PreGoalFork { goal_id: GoalId },           // 12-multi-agent-coordination.md
}

struct ActionRecord {
    action_id: ActionId,
    intent_id: IntentId,               // 05-intent-engine.md
    agent_id: Option<AgentId>,
    agent_run_id: Option<AgentRunId>,  // 11-agent-runtime.md
    capability_id: CapabilityId,       // which Capability performed the write
    recovery_point_before: RecoveryPointId,
    objects_touched: Vec<ObjectId>,
    inverse_op: Option<InverseOperation>,  // symbolic inverse, when one exists
    prev_hash: Hash,                   // hash-chains the journal; tamper-evident
    status: ActionStatus,              // Committed | InFlight | Aborted
}

enum UndoScope {
    SingleAction(ActionId),
    Session { session_id: SessionId, back_to: Option<ActionId> },
    AgentRun(AgentRunId),
    Goal(GoalId),                      // 12-multi-agent-coordination.md, multi-agent atomic scope
    Global(RecoveryPointId),           // full restore-to
}

struct RetentionPolicy {
    horizon_by_class: HashMap<RetentionClass, Duration>,
    compaction_interval: Duration,
    never_gc_if_pinned_or_referenced: bool,  // always true; not actually configurable
}
```

## Algorithms

**Consistent cross-store snapshot creation.** Taking a recovery point is a lightweight two-phase
protocol, not a stop-the-world pause: (1) the Recovery Point Service requests a hybrid-logical-clock
timestamp `t` and asks each of the four stores to record "my snapshot reference as of `t`" — for
the blob store this is instantaneous (immutable content-addressing means there is nothing to
freeze); for the graph, vector, and metadata stores it is a native MVCC/generation cut, which is
also non-blocking for concurrent readers and writers past `t`; (2) once all four confirm, the
`RecoveryPoint` record is committed to the ledger. If any store fails to confirm within a bounded
timeout, the whole recovery point is discarded rather than partially recorded — an unconfirmed
recovery point must never be a candidate restore target (§Failure Modes).

**Undo resolution.** Given an `UndoScope`, the Undo Coordinator first tries the *targeted* path:
compute the minimal set of `ActionRecord`s in scope, and if every one has a declared symbolic
`inverse_op` and none of the objects they touched has been further modified by an unrelated actor
since, apply the inverses in reverse chronological order — this is cheap and, critically, does not
disturb any concurrent, unrelated edit to a shared Semantic Object. If any touched object has been
modified since by something outside the undo scope (a classic shared-workspace conflict), the
Coordinator falls back to a *preview-and-confirm* restore: it computes what a full
`restore_to(recovery_point_before)` would discard beyond the intended scope and surfaces that as an
explicit choice, never silently overwriting an unrelated party's subsequent work.

**Multi-agent atomic goal rollback.** [12 — Multi-Agent Coordination](12-multi-agent-coordination.md)
takes a `PreGoalFork` recovery point at the moment a goal is decomposed across multiple Agents.
Because every sub-Agent's `ActionRecord`s are tagged with the same `goal_id`, an aborted or
crashed goal resolves via `UndoScope::Goal(goal_id)`: the Coordinator restores to the single
pre-goal-fork recovery point, treating the goal's cumulative effect as one atomic unit — analogous
to a distributed transaction abort. This is deliberate and non-negotiable: partially undoing only
the Agent whose sub-task failed, while leaving sibling Agents' writes standing, can leave the
shared goal state referencing objects that no longer have the dependencies the goal's plan assumed
they would.

**Crash recovery mid-Agent-execution.** On restart after an ungraceful shutdown, the Recovery
Point Service replays the Action/Intent Journal forward from the last *committed* recovery point.
Any `ActionRecord` with `status: InFlight` — meaning the write was journaled as started but never
reached a terminal `Committed`/`Aborted` state — is treated as unsafe: it is not replayed forward
(that risks re-executing a side effect twice) and it is not assumed complete. Instead, the objects
it names are restored to their state as of `recovery_point_before`, and [11 — Agent
Runtime](11-agent-runtime.md) is notified to re-plan that Agent's step from clean Intent state
rather than resume a state whose completeness is unknown. This is the write-ahead-log discipline
applied to Agent execution rather than only to storage.

**Retention and compaction.** Recovery points age through retention classes (e.g., dense for the
last hour, hourly for the last day, daily for the last month, per user-configured
`RetentionPolicy`). Compaction merges the *deltas between* adjacent unpinned recovery points into
a coarser interval, but — this is the one hard invariant of the whole subsystem — it never
discards a recovery point that is pinned, that is the sole path to satisfying a still-open
`ActionRecord.recovery_point_before` reference, or that an audit or legal-hold requirement from
[18 — Explainability & Trust](18-explainability-and-trust.md) has flagged as referenced evidence.

## Interfaces / APIs

```
recovery_point_create(trigger: Trigger) -> RecoveryPointId
recovery_point_list(filter: ObjectId | AgentRunId | GoalId | TimeRange) -> Vec<RecoveryPoint>
undo(scope: UndoScope) -> UndoReceipt
redo(scope: UndoScope) -> RedoReceipt
restore_to(recovery_point_id: RecoveryPointId, dry_run: bool) -> RestorePreview | RestoreReceipt
pin(recovery_point_id: RecoveryPointId)
unpin(recovery_point_id: RecoveryPointId)
retention_policy_set(policy: RetentionPolicy)
```

`undo` and `restore_to` are themselves capability-secured, auditable operations
([02 §5](02-core-architecture.md#5-capability-security-as-the-unifying-security-model)): an Agent
may undo its *own* `AgentRun` scope as part of graceful self-correction, but undoing another
Agent's or another user's actions requires the same explicit grant any cross-boundary operation
requires. Every call here itself produces an `ActionRecord` — undo is not exempt from being
undoable (§Failure Modes).

## Pseudocode

```rust
/// Undo Coordinator: resolve a scope to either a targeted replay or a gated full restore.
fn undo(scope: UndoScope) -> UndoReceipt {
    let records: Vec<ActionRecord> = journal::records_in_scope(&scope);  // reverse-chron order
    if records.is_empty() {
        return UndoReceipt::NothingToUndo;
    }

    let anchor_rp = records.last().unwrap().recovery_point_before;

    let concurrently_modified = records.iter()
        .flat_map(|r| &r.objects_touched)
        .filter(|obj| store::modified_since(obj, anchor_rp, /*excluding*/ &scope))
        .collect::<Vec<_>>();

    if concurrently_modified.is_empty() && records.iter().all(|r| r.inverse_op.is_some()) {
        // Targeted path: cheap, surgical, cannot disturb unrelated concurrent edits.
        for record in &records {
            apply_inverse(record.inverse_op.as_ref().unwrap());
            journal::append(ActionRecord::undo_of(record));   // undo is itself journaled
        }
        return UndoReceipt::Targeted { reversed: records.len() };
    }

    // Fallback path: a full restore would also discard unrelated concurrent work.
    let preview = restore_to(anchor_rp, /*dry_run*/ true);
    UndoReceipt::NeedsConfirmation(preview)   // surfaced to the user, never applied silently
}

/// Crash recovery: replay the journal forward from the last committed recovery point.
fn recover_from_crash() {
    let last_committed = recovery_point_ledger::last_committed();
    let tail = journal::records_since(last_committed);

    for record in tail {
        match record.status {
            ActionStatus::Committed => { /* durable; nothing to do */ }
            ActionStatus::Aborted   => { /* already rolled back at write time */ }
            ActionStatus::InFlight  => {
                // Unknown completeness: never replay forward, never assume success.
                restore_objects(&record.objects_touched, record.recovery_point_before);
                agent_runtime::replan(record.agent_run_id);   // 11-agent-runtime.md
            }
        }
    }
}
```

## Security Considerations

Recovery points retain full historical content, including for objects since deleted by the user —
which means they inherit, without exception, the strictest access control and encryption policy
of the underlying object at the time it existed ([16 — Privacy Architecture](16-privacy-architecture.md));
a pinned or long-retained recovery point is exactly as sensitive as the live data it shadows, and
is not a side channel around a subsequent permission downgrade or deletion request. The Action/
Intent Journal is hash-chained (`prev_hash`) specifically so that a compromised Capability cannot
retroactively falsify what it did — tampering with any entry breaks the chain from that point
forward, which [17 — Threat Model](17-threat-model.md) treats as a first-class detectable event.
Undo and restore are capability-gated operations in their own right (§Interfaces/APIs): a
compensating action replay must never silently re-acquire authority the kernel's revocation graph
has since revoked ([03 — Kernel Architecture §Algorithms](03-kernel-architecture.md#algorithms)) —
`apply_inverse` re-checks live capability tokens exactly as the original action did, rather than
assuming the original authorization is still valid. Because triggering an unnecessary recovery
point before every trivial write would itself become a privacy liability (more historical data
retained than needed), automatic recovery point creation is deliberately risk-weighted
(§Trade-offs), coordinated with [15 — Security Architecture](15-security-architecture.md)'s
risk-assessment engine rather than applied uniformly.

## Failure Modes

- **Partial snapshot** — one of the four stores fails to confirm within the timeout. The whole
  recovery point is discarded, not partially recorded; nothing ever restores to a `RecoveryPoint`
  that lacks all four store references.
- **Conflicting restore target** — objects in scope were modified by an unrelated actor since the
  anchor recovery point. Surfaced as `UndoReceipt::NeedsConfirmation` rather than silently
  discarding the unrelated edit (§Algorithms, §Pseudocode).
- **Cascading undo into legitimate subsequent work** in a shared Workspace — mitigated by the same
  conflict detection; a full restore is never applied without an explicit preview-confirm step
  once concurrent modification is detected.
- **Crash during undo itself** — `undo`'s own effects are journaled as `ActionRecord`s
  (`journal::append(ActionRecord::undo_of(...))`), so an interrupted undo is itself subject to the
  crash-recovery algorithm above; there is no privileged, unrecoverable "meta" operation.
- **Corrupted recovery point ledger** — the ledger is itself checksummed and replicated across the
  Storage Engine's redundancy scheme ([28 — Storage Engine](28-storage-engine.md)); a corrupted
  entry is detected by hash-chain verification and the last verifiably intact recovery point
  becomes the effective floor for any restore.
- **Retention/compaction destroying a still-needed point** — structurally prevented, not merely
  discouraged: compaction is required to prove a candidate recovery point is unreferenced by any
  pinned point, open `ActionRecord`, or audit hold before it is eligible for merge (§Algorithms).

## Recovery Mechanisms

The mechanisms above *are* Hyperion's system-wide recovery mechanism — this document is deliberately
the terminus other documents point to rather than one more consumer of a mechanism defined
elsewhere. Three integrations are worth stating explicitly because they are named in this
document's brief: (1) [32 — Update System](32-update-system.md) calls `recovery_point_create(Trigger::PreUpdate(...))`
before staging any Capability or model change, and reverts those via a plain `restore_to` call
covering both the change and any data migration it performed, since the recovery point spans both
by construction. System-image reversal is the one exception: it never calls `restore_to` at all,
reverting instead via 32's own bootloader slot-pointer flip; if that image shipped alongside a
data migration, 32 issues the slot flip *and* a separate `restore_to` call together as one
user-facing rollback, not a single primitive doing both (32 §Recovery Mechanisms). (2) [12 — Multi-Agent Coordination](12-multi-agent-coordination.md) relies on
`UndoScope::Goal` for atomic multi-agent rollback, so a crashed or abandoned shared goal never
leaves the Knowledge Graph half-updated by one Agent and untouched by its siblings. (3)
[15 — Security Architecture](15-security-architecture.md)'s risk-assessment engine calls
`recovery_point_create(Trigger::PreRiskyAction)` synchronously, in the request path, before any
action it classifies as high-risk is allowed to execute — the recovery point is a precondition of
execution, not a best-effort afterthought, which is the literal reading of
[02 §4](02-core-architecture.md#4-design-invariants) invariant 2.

## Performance Analysis

Because store-native snapshot primitives (content-addressed references, MVCC cuts) require no
data copying, taking a recovery point is designed to be a low-single-digit-millisecond metadata
operation regardless of how much data the object graph contains — the cost of a recovery point is
O(number of stores), not O(data size). Storage cost of retained recovery points is O(changes
between points), since only deltas between snapshot references are retained after compaction, not
full copies at every point. Targeted undo (the common case) is bounded by the number of
`ActionRecord`s in scope, typically small; full restore is bounded by the size of the delta between
the current state and the target recovery point, which grows with how far back the undo reaches —
this is the direct reason undo depth and retention horizon are governed by the same policy
(§Trade-offs). Crash recovery's journal replay is bounded by the time since the last committed
recovery point, which [15 — Security Architecture](15-security-architecture.md)'s risk-weighted
creation policy keeps small for any Agent run doing consequential work.

## Trade-offs

Fine-grained, per-action journaling with symbolic inverses enables cheap, surgical undo that does
not disturb unrelated concurrent work, but it requires every Capability to declare an inverse
operation where one exists — additional authoring burden on Capability developers
([24 — Plugin Framework](24-plugin-framework.md), [25 — SDK](25-sdk.md)) relative to a
coarser design that only ever restores whole snapshots. Retaining recovery points indefinitely
would maximize undo depth but directly conflicts with both storage budget and the data-minimization
posture of [16 — Privacy Architecture](16-privacy-architecture.md); Hyperion resolves this with the
tiered `RetentionPolicy` (§Data Structures) rather than either extreme. The most consequential
trade-off named in this document's brief is automatic-versus-selective recovery point creation:
creating a synchronous recovery point before *every* state-changing operation would make undo
depth unbounded but would impose a per-write latency and storage tax on the overwhelming majority
of low-consequence actions (a keystroke-level edit does not need the same guarantee as an
irreversible file deletion or a system update). Hyperion instead ties automatic, synchronous
recovery point creation to [15 — Security Architecture](15-security-architecture.md)'s risk
classification — high-risk and explicitly flagged actions get a guaranteed, blocking recovery
point; routine low-risk actions are covered by cheaper, asynchronously batched checkpoints on a
short interval, trading a small, bounded undo gap for materially lower overhead on the common
path.

## Testing Strategy

Chaos tests kill the Recovery Point Service mid-snapshot (after some stores confirm, before
others) and assert no partial `RecoveryPoint` is ever visible to `restore_to`. Journal replay
determinism is verified by re-running the crash-recovery algorithm against recorded journals from
real Agent-run failures and asserting a byte-identical resulting state across repeated runs.
Multi-agent rollback integration tests specifically construct a shared goal with sibling Agents at
different completion points, kill the coordinating process mid-goal, and assert
`UndoScope::Goal` restores every participant's state to the pre-fork recovery point, not a mix of
undone and standing writes. Retention/compaction property tests assert, as an invariant checked on
every compaction run, that no pinned or still-referenced recovery point is ever garbage collected.
Finally, rollback drills required by [32 — Update System §Testing Strategy](32-update-system.md#testing-strategy)
exercise this document's `restore_to` path directly as part of every release pipeline run, so the
"never an inconsistent Knowledge Graph" guarantee is a continuously-verified property of the system
rather than a design aspiration.

---
*Next: [34 — Observability & Telemetry](34-observability-telemetry.md).*
