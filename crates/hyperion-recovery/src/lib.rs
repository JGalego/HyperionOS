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
//! Deliberately deferred, and why:
//!
//! - **A true zero-copy, whole-graph MVCC cut.** docs/33 frames a
//!   `RecoveryPoint` as "not a copy of data; a durable, timestamped
//!   reference" because the real store's native MVCC/content-addressing
//!   makes a reference cheap. `hyperion-knowledge-graph` doesn't expose a
//!   historical-version read API (only the *current* value per node), so
//!   this crate captures the current value of exactly the objects an
//!   action declares up front — a real copy, bounded by that action's
//!   scope, not the free omniscient reference the doc describes. Adding
//!   a historical-read API to `hyperion-knowledge-graph` is deferred.
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
//! - **`UndoScope::Session`/`UndoScope::Goal`.** Neither concept has a
//!   first-class id anywhere in this workspace yet (`hyperion-
//!   coordination`'s `SharedPlan` has no single "goal id" distinct from
//!   its task graph) — `SingleAction`/`AgentRun`/`Global` cover every
//!   scope this crate's callers can actually name today.
//! - **Retention classes, compaction, and pinning enforcement beyond a
//!   boolean flag.** [`service::RecoveryService::pin`]/`unpin` exist;
//!   nothing yet reads that flag to protect a point from eviction, since
//!   this crate has no eviction/compaction pass at all yet — recovery
//!   points and the action journal simply accumulate for the process
//!   lifetime, appropriate for a hosted simulator with no long-running
//!   retention story yet.
//! - **Redo.** docs/33 lists a `redo(scope)` API; this crate's undo marks
//!   undone actions `Aborted` rather than keeping a separate redo stack,
//!   so redo is not implemented.

mod service;
mod types;

pub use service::RecoveryService;
pub use types::{
    ActionId, ActionRecord, ActionStatus, RecoveryError, RecoveryPoint, RecoveryPointId, Trigger,
    UndoReceipt, UndoScope,
};
