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
//! - **Progress/escalation broadcast over a real Event System**
//!   ([31 — Event System](../31-event-system.md), not built) —
//!   [`CoordinationSession::progress`] and `.escalations()` are pull-based
//!   accessors a caller polls, not a push subscription.
//! - **Object-affinity plan partitioning** (docs/12 §12, a scale
//!   optimization for tens of concurrent Agents) — this crate's
//!   `SharedPlan` is one unpartitioned structure, adequate at this phase's
//!   test scale.
//! - **A workspace-wide, shared Explanation Record store.** This
//!   session's `ExplanationStore` is private to one `CoordinationSession`,
//!   not shared with `hyperion-api-gateway`'s own separate store or
//!   `hyperion-federation`'s own separate one (that crate's
//!   `dispatch_offload`/`invoke_agent` now open real records too, just
//!   into their own store) — a follow-up for whichever future slice
//!   needs one workspace-wide trace
//!   rather than several independent ones.

mod catalog;
mod engine;
mod types;

pub use catalog::{default_manifests, required_capabilities_for};
pub use engine::{CoordError, CoordinationSession};
pub use types::{
    AllocationRecord, ConflictKind, ConflictRecord, ConflictResolution, Escalation, SharedPlan,
    TaskNode, TaskStatus, WriteOutcome,
};
