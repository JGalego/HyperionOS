//! Hyperion L1/L3 Rollback & Recovery — Phase 8, first slice.
//!
//! Implements docs/33-rollback-recovery.md's recovery-point/undo/crash-
//! recovery machinery on top of `hyperion-knowledge-graph`, per Phase 8's
//! own framing in docs/41-implementation-phases.md as a hardening pass:
//! the WAL and Agent checkpoint/resume this workspace already has are the
//! primitives; this crate is where they gain a real coordination layer.
//!
//! Real: [`service::RecoveryService::recovery_point_create`] captures a
//! bounded snapshot of exactly the objects a triggering action is about
//! to touch (see below for why this deviates from docs/33's "reference,
//! not a copy" framing); [`service::RecoveryService::record_action_started`]
//! /`record_action_committed`/`record_action_aborted` implement docs/33
//! §4's `ActionRecord` journal; [`service::RecoveryService::undo`]
//! resolves docs/33 §5's conflict-detection pseudocode exactly (restore
//! directly if nothing outside scope touched the same objects since,
//! else surface conflicts and require explicit confirmation — never a
//! silent overwrite); [`service::RecoveryService::recover_from_crash`]
//! implements docs/33 §5's exit-criterion algorithm: every action
//! journaled `InFlight` and never closed is rolled back to its pre-
//! action snapshot, never replayed forward, then its Agent instance is
//! terminated and a fresh instance spawned against the same manifest and
//! bound Intent — this crate's translation of "hands control back to
//! Agent Runtime to re-plan, not resume mid-step" (re-planning itself
//! belongs to [05 — Intent Engine](../05-intent-engine.md), invoked
//! whenever a caller next drives the fresh instance).
//!
//! Real (2026-07-16, docs/998-roadmap.md's Self-Sustaining pillar): this crate's own rollback
//! machinery used to be purely reactive, with "no mechanism connects a rollback's cause to a
//! future decision." [`service::RecoveryService::restore_to_with_cause`] closes that for real:
//! an optional, real [`hyperion_memory::MemoryEngine`] (`Option<Arc<...>>`, the same shape
//! `hyperion-agent-runtime`'s own optional backends already use) remembers a real
//! [`types::RollbackCause`] — a short reason plus whatever structured data justified it — in its
//! Procedural tier every time a caller rolls back with one. [`service::RecoveryService::rollback_causes`]
//! really queries that history back. `hyperion-update::orchestrator::UpdateOrchestrator` is the
//! real caller: it now threads its own previously-discarded health-breach data into the cause
//! it rolls back with, and refuses to retry an update it just rolled back for the exact same
//! reason — a rollback's cause now really shapes a future decision, not just a future log line.
//!
//! Deliberately deferred, and why:
//!
//! - **A true zero-copy, whole-graph MVCC cut.** docs/33 frames a
//!   `RecoveryPoint` as "not a copy of data; a durable, timestamped
//!   reference" because the real store's native MVCC/content-addressing
//!   makes a reference cheap. `hyperion-knowledge-graph` now has a real
//!   historical-version read API (`KnowledgeGraph::current_version`/
//!   `get_at_version`, layered directly on `hyperion-storage`'s own
//!   version chain) — this crate's own gap named it as the reason a real
//!   reference-based `RecoveryPoint` didn't exist; that reason is gone.
//!   Still deliberately not rewired to use it here, though: this crate's
//!   snapshot-capture is already real, tested, and "cheap at the scale of
//!   one action's `objects_touched`" per its own doc above, and switching
//!   `RecoveryPoint`/`ActionRecord` from an owned copy to a
//!   `(NodeId, VersionId)` reference is a real, separate representation
//!   change to this crate's own types and every method that reads them
//!   (`restore_objects`, `undo`, `redo`, `recover_from_crash`), not a
//!   one-line wiring fix — worth its own careful pass rather than folding
//!   into an unrelated change.
//! - **Un-creating a freshly created object.** `hyperion-knowledge-graph`
//!   has no node-delete operation (only edges tombstone); a recovery-
//!   point snapshot of an object that didn't exist yet is recorded as
//!   `None` and is simply not restorable — this crate's `restore`
//!   reverts *modifications* to pre-existing objects, never *creations*.
//! - **`InverseOperation`/symbolic inverses** (docs/33 §4's
//!   `ActionRecord.inverse_op`) — every "inverse" here is the literal
//!   pre-action snapshot restored verbatim, not a separately-declared
//!   symbolic operation. Simpler, and sufficient for every mitigation
//!   this crate's tests exercise.
//! - ~~`UndoScope::Session`/`UndoScope::Goal`~~ (2026-07-16) — now real, closing this crate's own
//!   named gap: `hyperion_coordination::types::SharedPlan` had a real `session_id: u64` and
//!   `root_intent: NodeId` before this crate had a way to key on them.
//!   [`service::RecoveryService::record_action_started_with_scope`] tags an `ActionRecord` with
//!   both (`record_action_started` still tags neither, for every caller with no session/goal
//!   concept of its own); `hyperion-coordination::CoordinationSession::with_recovery` is the real,
//!   optional caller (`Option<Arc<...>>`, this workspace's own established shape) — every real
//!   task dispatch its `allocate()` completes now opens a real, best-effort recovery point +
//!   `ActionRecord` around the real `"task_result"` node it creates, tagged with that session's
//!   own real `session_id`/`root_intent`. Honest scope: `"task_result"` is always a *fresh* node,
//!   so this specific action's own undo can't restore it (the "un-creating a freshly created
//!   object" limitation named below already applies) — the real value landed here is genuine
//!   crash-recovery journaling and session/goal-scoped bookkeeping, proven end to end in both
//!   crates' own test suites (`hyperion-recovery`'s `tests/undo_scope_session_goal.rs`;
//!   `hyperion-coordination`'s `tests/recovery_bridge.rs`).
//! - **Retention classes, compaction, and pinning enforcement beyond a
//!   boolean flag.** [`service::RecoveryService::pin`]/`unpin` exist;
//!   nothing yet reads that flag to protect a point from eviction, since
//!   this crate has no eviction/compaction pass at all yet — recovery
//!   points and the action journal simply accumulate for the process
//!   lifetime, appropriate for a hosted simulator with no long-running
//!   retention story yet.
//! - ~~**Redo.**~~ Now real: [`service::RecoveryService::undo`] captures
//!   each reverted action's actual pre-revert state (its real committed
//!   effects) into a redo snapshot, distinct from marking it `Aborted`;
//!   [`service::RecoveryService::redo`] re-applies exactly that captured
//!   state, gated by the same "surface conflicts, never silently
//!   overwrite concurrent work" rule `undo` already uses. `ActionStatus`
//!   gained a fourth variant, `Undone`, distinct from `Aborted` (an
//!   action that never took effect has nothing to redo; one that ran and
//!   was reverted does).

mod service;
mod types;

pub use service::RecoveryService;
pub use types::{
    ActionId, ActionRecord, ActionStatus, RecordedRollback, RecoveryError, RecoveryPoint,
    RecoveryPointId, RedoReceipt, RollbackCause, Trigger, UndoReceipt, UndoScope,
};
