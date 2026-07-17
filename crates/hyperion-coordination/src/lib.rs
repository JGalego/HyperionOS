//! Hyperion L5 Multi-Agent Coordination — Phase 4, second and final slice.
//!
//! Implements docs/12-multi-agent-coordination.md's answer to "given an
//! Intent Graph too large for one Agent, who does what, sharing what state,
//! and who tells the user how it's going" — built directly on the real
//! [`hyperion_intent::IntentEngine`] (an Intent Graph's leaves become
//! [`TaskNode`]s) and the real [`hyperion_agent_runtime::AgentRuntime`]
//! (allocation spawns and invokes real [`hyperion_agent_runtime::AgentInstance`]s).
//! This is the top of the Phase 1-4 stack: every crate built so far in this
//! workspace is load-bearing underneath a call to [`CoordinationSession::allocate`].
//! [`CoordinationSession::create_session`] takes a real
//! `hyperion_intent::ExecutionTicket` (from `IntentEngine::submit`), not a
//! bare `NodeId` — `hyperion-intent`'s own doc comment named that hand-off
//! as never actually happening ("nothing actually assigns or dispatches an
//! Agent to them yet"); requiring the real ticket here makes it a real,
//! enforced precondition rather than an unused, parallel API.
//!
//! Real: capability-and-trust-tier-gated task allocation (§5.1) — trust is
//! a hard eligibility filter, never a scoring input; optimistic-concurrency
//! `propose_write` with auto-merge for non-overlapping keys and a real
//! `ConflictRecord` for genuine collisions (§5.2); a weighted progress
//! rollup (§5.3); and failure containment (§5.4) — a failed `TaskNode`'s
//! dependents are marked `Blocked`, reallocation is retried once with a
//! fresh Agent instance, and a second failure produces a named
//! [`Escalation`] rather than a silent stall. The docs/41 Phase 4 exit
//! criterion — the "Launch my product" trace running end-to-end against
//! stub Capabilities, with a deliberately-failed Agent contained without
//! corrupting the shared goal state — is this crate's own integration test.
//! Every real dispatch [`engine::CoordinationSession::allocate`] makes also
//! opens a real `hyperion-explainability` Explanation Record — `begin`
//! before dispatch, a `ReasoningStep` naming the assigned Agent and task,
//! `transition` to `Completed`/`RolledBack`/`Interrupted` depending on the
//! real [`hyperion_agent_runtime::InvokeOutcome`] — closing the gap
//! `hyperion-explainability`'s own doc comment names (this crate's
//! `allocate` specifically) rather than `hyperion-agent-runtime::invoke`
//! itself, since that lower-level crate can't depend on
//! `hyperion-explainability` without a real dependency cycle through
//! `hyperion-recovery` (which depends on `hyperion-agent-runtime` for
//! crash-recovery reconciliation).
//!
//! Deliberately deferred, and why:
//!
//! - **Claim vs. execute as separate steps.** docs/12 §5.1 separates
//!   `claimTask` (reserve) from actually starting execution once
//!   dependencies clear. This crate's [`CoordinationSession::allocate`]
//!   fuses claim-and-immediately-invoke into one synchronous pass, because
//!   there is no real asynchronous Agent reasoning loop yet deciding *when*
//!   to start a claimed task — see `hyperion-agent-runtime`'s own deferral
//!   of real reasoning state.
//! - **Contradictory-subplan detection is manual, not automatic.** docs/12
//!   §5.2's "two `TaskNode`s' stated assumptions about a shared fact
//!   diverge" needs semantic comparison of Agent outputs this crate has no
//!   model to perform; [`CoordinationSession::flag_contradiction`] lets a
//!   caller (standing in for whatever *would* detect the divergence) raise
//!   one directly, and the same Coordinator-arbitration/escalation ladder
//!   (§5.2) runs from there.
//! - **A production IPC transport call site.** All coordination calls
//!   here are still direct method calls on one in-process
//!   [`CoordinationSession`], not `CoordMessage`s over a real production
//!   transport — consistent with how `hyperion-context`'s
//!   `ContextPropagation` treats transport as out of scope, and for the
//!   same reason: no real production caller drives coordination across a
//!   real process boundary yet. What *is* now proven, dev-dependency-only
//!   in `tests/ipc_transport.rs`: `propose_write`'s message shape
//!   (`session_id`/`agent_instance`/`key`/`base_version`/`value`, and the
//!   real `WriteOutcome` it returns) genuinely survives a real
//!   `hyperion-ipc` `CALL` frame between two separate Trust Boundaries,
//!   applied for real against a live session on the receiving side —
//!   [`WriteOutcome`]/[`ConflictRecord`]/[`ConflictKind`]/[`ConflictResolution`]
//!   all gained real `Serialize`/`Deserialize` impls to make that
//!   possible.
//! - ~~**Progress/escalation broadcast over a real Event System**~~ — now
//!   real: [`engine::CoordinationSession::with_events`] wires a real
//!   `hyperion-events::EventBus`; [`engine::CoordinationSession::allocate`]
//!   publishes a real `AgentProgress`/`coordination.task_progress.v1`
//!   event on every task's `Done` transition, and a real `AgentProgress`/
//!   `coordination.escalation.v1` event whenever a real
//!   [`types::Escalation`] is raised (both in `allocate`'s own failure path
//!   and in [`engine::CoordinationSession::arbitrate_contradiction`]).
//!   [`engine::CoordinationSession::progress`]/`.escalations()` remain —
//!   pull and push are complementary, not a replacement for each other.
//! - ~~Object-affinity plan partitioning~~ (docs/12 §12, a scale
//!   optimization for tens of concurrent Agents) — now real:
//!   [`engine::task_partition_key`] groups tasks into the same real
//!   partition exactly when real `TaskNode::dependencies` edges connect
//!   them, transitively (docs/12 §12's own worked example: Documentation
//!   vs. Deployment never share one). [`types::SharedPlan::partition_versions`]
//!   replaces the single, plan-wide `version` counter every task-status
//!   change previously bumped regardless of which task changed —
//!   confirmed dead (nothing in this crate, or anywhere in this workspace,
//!   ever read it) before being replaced, not merely shadowed —
//!   with one real counter per partition, bumped only by
//!   [`engine::CoordinationSession::allocate`]/[`engine::CoordinationSession::amend_task`]'s
//!   own real task-status changes and readable via the new
//!   [`engine::CoordinationSession::partition_version`]. `propose_write`'s
//!   plan facts needed no equivalent change — they already carried a real,
//!   independent per-key version, proven by this crate's own existing
//!   `writes_to_different_keys_never_conflict` test.
//! - ~~A workspace-wide, shared Explanation Record store~~ — now real for a caller that wants
//!   it: [`engine::CoordinationSession::new_with_shared_explanations`] takes a real, caller-supplied
//!   `Arc<ExplanationStore>` instead of building its own private one, the same real store a
//!   `hyperion_federation::FederationHub` built via its own
//!   `new_with_shared_explanations` (or a `hyperion-api-gateway::ApiGateway`, which already took
//!   one) can share too. Every real `action_id` this session mints now comes from the store's own
//!   `ExplanationStore::next_action_id` rather than a private, owner-local counter — sharing a
//!   store without also sharing that counter would let two independent owners' `action_id`s
//!   collide; `hyperion-federation`/`hyperion-api-gateway` made the identical change the same
//!   pass. `CoordinationSession::new` is unchanged (still builds its own private store; every
//!   existing call site keeps compiling). Proven end to end, cross-crate: see
//!   `tests/shared_explanation_store.rs` — a real `CoordinationSession` and a real, genuinely
//!   independent `FederationHub`, sharing one store, each contribute a real record under the same
//!   real Intent id with no `action_id` collision.
//!
//! Real (2026-07-16, docs/998-roadmap.md's Backlog "Protect the Human" item): "no declared
//! judgment/taste/empathy/context boundary, distinct from 'risky'" — [`catalog::judgment_class_for`]
//! really classifies a task predicate (the item's own worked example: `"branding"` is
//! [`types::JudgmentClass::TasteOrEmpathy`], `"legal_formation"` is
//! [`types::JudgmentClass::Mechanical`]), [`create_session`](engine::CoordinationSession::create_session)
//! stamps it onto each real [`types::TaskNode`], and [`allocate`](engine::CoordinationSession::allocate)
//! appends a real, second `ReasoningStep` to that dispatch's Explanation Record for a
//! `TasteOrEmpathy` task — advisory only, per CLAUDE.md's User Control principle: this never
//! changes dispatch, routing, or eligibility, only names a reason a human-facing surface might
//! choose to ask for more direct involvement regardless of reversibility.
//!
//! Real (2026-07-16): `hyperion-recovery`'s own previously-named "`UndoScope::Session`/
//! `UndoScope::Goal`" gap — "neither concept has a first-class id anywhere in this workspace" was
//! false the moment `SharedPlan.session_id`/`root_intent` existed; what was missing was a real
//! caller tagging an `ActionRecord` with them. [`engine::CoordinationSession::with_recovery`]
//! is that real, optional caller: every real task dispatch [`engine::CoordinationSession::allocate`]
//! completes now opens a real, best-effort `hyperion_recovery::RecoveryService` recovery point +
//! `ActionRecord` around the real `"task_result"` node it creates, tagged with this session's own
//! real `session_id`/`root_intent` — undoable later via `UndoScope::Session`/`UndoScope::Goal`.
//! See that method's own doc comment for the honest scope boundary (a fresh node, so this
//! specific action's own undo can't restore it — the real value is crash-recovery journaling and
//! session/goal-scoped bookkeeping).

mod catalog;
mod engine;
mod types;

pub use catalog::{
    best_fit_manifest_with_plugins, default_manifests, judgment_class_for,
    required_capabilities_for,
};
pub use engine::{CoordError, CoordinationSession};
pub use types::{
    AllocationRecord, ConflictKind, ConflictRecord, ConflictResolution, Escalation, JudgmentClass,
    SharedPlan, TaskNode, TaskStatus, WriteOutcome,
};
