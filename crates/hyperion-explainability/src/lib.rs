//! Hyperion L3/L4-cross-cutting Explainability & Trust — Phase 8, fourth
//! slice.
//!
//! Implements docs/18-explainability-and-trust.md's `ExplanationRecord`
//! system: the Phase 8 exit criterion is "every autonomous action across
//! Phases 3-7 produces a queryable Explanation Record" — this crate is
//! the queryable store and query surface those actions attach to; wiring
//! every Phase 3-7 crate's call sites to actually call
//! [`store::ExplanationStore::begin`] is left to each of those crates
//! (none of them are touched by this slice — see this crate's doc
//! comment).
//!
//! Real: [`store::ExplanationStore`] implements docs/18 §5's explain-
//! then-commit ordering — a record is opened with [`store::ExplanationStore::begin`]
//! *before* any reasoning step runs, appended to as reasoning happens
//! (never reconstructed after the fact), and only reaches
//! `ControlState::Completed` after the real effect finishes;
//! [`store::ExplanationStore::incomplete`] is docs/18 §9's completeness
//! invariant made queryable directly, rather than a background checker
//! this crate doesn't run; [`store::ExplanationStore::link_parent`]
//! implements docs/18 §5's multi-agent composition DAG;
//! [`render::resolve_why`] is docs/18 §5/§6's `resolve_why`/`explain.query`
//! — resolves by `action_id` alone (never requires already knowing an
//! internal record id), and at `Depth::Full` walks `parent_records`
//! depth-first exactly as the doc's multi-agent merge describes.
//! `undo_ref`/`privacy_class` are typed as [`types::RecoveryPointId`]/[`types::SensitivityClass`]
//! — narrowed local copies of `hyperion-recovery`'s `RecoveryPointId` (a bare `u64` alias, so
//! values pass through unchanged) and `hyperion-privacy`'s `SensitivityClass`, not those crates'
//! own types directly. This crate's own previously-named "agent-runtime/explainability Cargo
//! cycle" gap depended on exactly this: `hyperion-recovery` depends on `hyperion-agent-runtime`
//! (for its own real crash-recovery re-spawn), and `hyperion-privacy` depends on
//! `hyperion-recovery` (for its own real erasure grace period) — so this crate depending on
//! either, for nothing more than a `u64` alias and a 4-variant enum, would have made
//! `hyperion-agent-runtime` depending on *this* crate a hard Cargo cycle. Narrowing both types
//! locally (the same precedent `hyperion-security::SensitivityHint` already established) breaks
//! that cycle for real, and `hyperion_agent_runtime::AgentRuntime::with_explainability` is the
//! new real caller it unblocks — see that crate's own doc comment.
//!
//! Deliberately deferred, and why:
//!
//! - **Wiring Phases 3-7's remaining call sites.** This crate is the
//!   store and query API; it does not itself instrument every already-
//!   shipped crate's decision points to call
//!   `begin`/`append_step`/`transition`. `hyperion-coordination`'s
//!   `allocate`, `hyperion-federation`'s `dispatch_offload`/`invoke_agent`,
//!   and `hyperion-intent`'s HTN decomposition are now wired (each defaults
//!   to its own private `ExplanationStore`, or can share one real store
//!   with the others — see [`store::ExplanationStore::next_action_id`]'s
//!   own doc comment, and those crates' own `new_with_shared_explanations`
//!   constructors); `hyperion-agent-runtime`'s `invoke` is not and cannot be
//!   the same way — a real Cargo dependency cycle
//!   (`hyperion-explainability` → `hyperion-recovery` →
//!   `hyperion-agent-runtime`) rules out a direct dependency, so that call
//!   site needs a different composition (a layer above both, the way
//!   `hyperion-federation` sits above `hyperion-agent-runtime`), not
//!   attempted here.
//! - **Real NLG / natural-language explanation text.**
//!   [`render::resolve_why`]'s headline is a deterministic template
//!   (`format!`), not a model call — docs/18 §6's `render_at_complexity_level`
//!   describes a *mechanism* (tiering by depth/user complexity level),
//!   not literal copy; this crate implements the mechanism's shape
//!   (`Depth::Headline`/`Depth::Full`) without a real model behind it.
//! - **`ConfidenceScore.method` implementations.** ~~`SelfConsistency`~~ (2026-07-16) — now real:
//!   [`self_consistency_confidence`] is docs/18 §9's own "self-consistency across repeated
//!   sampling" — calls a real, wired `hyperion-ai-runtime::LocalAiRuntime` with an identical
//!   prompt several real times and reports the real majority-answer agreement fraction, `None`
//!   (never a fabricated score) if any one of those real calls couldn't run. `Verifier`/
//!   `Ensemble` remain declared but uncomputed: `Verifier` needs real formal verification this
//!   workspace doesn't have, and `Ensemble` depends on
//!   [23 — Multi-Model Orchestration](../23-multi-model-orchestration.md)'s actual candidate
//!   models — neither is what [`self_consistency_confidence`] computes.
//! - ~~**`best_effort_reconstruction` via the Event System (31).**~~ — now real:
//!   [`ExplanationStore::with_events`] wires a real `hyperion-events::EventBus` and opens one
//!   long-lived, `Durable` admin subscription; `begin`/`append_step`/`transition` each publish a
//!   real event, and [`ExplanationStore::get_or_reconstruct`]/`get_or_reconstruct_by_action`
//!   replay that log to approximate a record docs/18 §9's degrade path describes, re-applying the
//!   same Trust-Boundary check [`ExplanationStore::get`] does before ever returning one. Honest
//!   scope: only what was actually published is recoverable — `evidence`/`confidence`/
//!   `alternatives`/`undo_ref`/the parent-child DAG are never reconstructed, and
//!   [`ExplanationView::reconstructed`] flags the result rather than presenting it as
//!   authoritative, exactly as docs/18 §9 requires.
//! - ~~**`control.interrupt`/`control.modify`/`control.resume` signal plumbing.**~~ — all three
//!   now real, delivered by `hyperion-coordination`: `CoordinationSession::apply_dispatch_results`
//!   already transitions a task's record to `Interrupted` when its real dispatch comes back
//!   `PendingConsent`/`QuotaExceeded` (a genuine "paused, waiting on something external," not a
//!   task failure); `CoordinationSession::amend_task` transitions the amended task's most recent
//!   real record to `Modified` the moment a real user amendment (`hyperion-console`'s own
//!   `/redo`) lands; `CoordinationSession::resume_task` — this crate's own previously-named "no
//!   scheduler or Agent Runtime hook actually delivers `control.resume`" gap, the last of the
//!   three — resets a genuinely `Interrupted` task back to real dispatch eligibility and
//!   transitions its record to `Executing`, refusing (a real, honest error, not a silent no-op)
//!   a task that isn't currently `Claimed`+`Interrupted` (already resumed, never interrupted, or
//!   terminally `Denied` — which also leaves a task `Claimed`, but with a `RolledBack` record).
//!   All three read/write the same real `last_explanation_by_task` map that survives past each
//!   dispatch's own terminal transition, since every real dispatch attempt mints its own fresh
//!   `ExplanationId` rather than reusing one record across attempts.
//! - ~~**Rolling Brier-score calibration tracking (§10).**~~ — now real:
//!   [`ExplanationStore::calibration_score`] computes a real rolling Brier score per
//!   `(agent_id, capability_ref)` pair over every terminal (`Completed`/`RolledBack`) record this
//!   crate already holds a real `confidence` for, and flags a real `alert` once that score
//!   crosses a real threshold with enough samples to trust the signal — see
//!   [`calibration`]'s own doc comment for the exact numbers. `Proposed`/`Executing`/
//!   `Interrupted`/`Modified` records have no real outcome yet to score against, matching
//!   [`ExplanationStore::incomplete`]'s own convention for what counts as resolved.
//! - **Encryption-at-privacy-tier of the store itself** (a `hyperion-
//!   privacy` dependency, not built here) — `privacy_class` is recorded
//!   on a record for a future consumer to act on, not enforced by this
//!   crate.
//!
//! ~~Every write here (`begin`/`append_step`/`set_confidence`/.../`transition`) was
//! capability-gated; every real read (`get`/`trace_intent`/`incomplete`/`calibration_score`,
//! and this crate's own `resolve_why` — its literal `explain.query` implementation) took no
//! `monitor`/`token` at all~~ (2026-07-17) — now real: docs/18 §8's own "access to an
//! `explain.query` result is gated by the same capability grant that gated the underlying
//! data — a user... cannot use the explanation channel as a side door to read data they were
//! never granted access to" is enforced for real. Every read requires `RightsMask::READ` and
//! filters by [`types::ExplanationRecord::trust_boundary_span`] — a record outside the
//! caller's own Trust Boundary is omitted/`None`, never an error that would reveal it exists.
//! [`store::ExplanationStore::begin`] now always seeds `trust_boundary_span` with the real,
//! live `token.origin()` (previously every real caller passed a dead, hardcoded `vec![]`, so
//! the field existed but nothing ever populated or read it back) — a caller's own explicit
//! span (docs/18 §5's multi-agent merge, where more than one boundary genuinely contributed)
//! is preserved and simply extended if it doesn't already include the boundary actually
//! opening the record. `hyperion-coordination::CoordinationSession::explanation`/
//! `hyperion-federation::FederationHub::explanation`/`trace_intent`/
//! `hyperion-intent::IntentEngine::explanation`/`trace_intent` — thin wrappers over this
//! crate's own store — all threaded the same capability check through, since each previously
//! re-exposed the identical ungated hole one layer up. This closes docs/18 §13's own explicit
//! call for "privacy regression tests... asserting `explain.query` never returns detail the
//! caller lacks a capability grant for," which did not exist until now (see
//! `tests/explain_query_capability_and_boundary.rs`).

mod calibration;
mod confidence;
mod render;
mod store;
mod types;

pub use confidence::self_consistency_confidence;
pub use render::resolve_why;
pub use store::ExplanationStore;
pub use types::{
    ActionId, Alternative, CalibrationScore, ConfidenceMethod, ConfidenceScore, ControlState,
    Depth, EvidenceRef, ExplainabilityError, ExplanationId, ExplanationLookup, ExplanationRecord,
    ExplanationView, ReasoningStep, RecoveryPointId, SensitivityClass,
};
