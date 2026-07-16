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
//! `undo_ref`/`privacy_class` reuse `hyperion-recovery`'s
//! `RecoveryPointId` and `hyperion-privacy`'s `SensitivityClass` directly
//! — composing already-real crates rather than re-mocking their
//! concerns, per this workspace's own convention.
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
//! - **`ConfidenceScore.method` implementations.** `SelfConsistency`/
//!   `Verifier`/`Ensemble` are declared as an enum a caller tags a score
//!   with; this crate computes none of them (`Ensemble` in particular
//!   depends on [23 — Multi-Model Orchestration](../23-multi-model-orchestration.md)'s
//!   actual candidate models).
//! - **`best_effort_reconstruction` via the Event System (31).** No Event
//!   System crate exists yet anywhere in this workspace to replay from;
//!   docs/18 §9's degrade path ("store unavailable → reconstruct, flagged
//!   non-authoritative") has nothing to reconstruct from yet.
//! - **`control.interrupt`/`control.modify`/`control.resume` signal
//!   plumbing.** [`types::ControlState`] has `Interrupted`/`Modified`
//!   variants a caller can transition a record into, but no scheduler or
//!   Agent Runtime hook actually delivers those signals from this crate.
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

mod calibration;
mod render;
mod store;
mod types;

pub use render::resolve_why;
pub use store::ExplanationStore;
pub use types::{
    ActionId, Alternative, CalibrationScore, ConfidenceMethod, ConfidenceScore, ControlState,
    Depth, EvidenceRef, ExplainabilityError, ExplanationId, ExplanationRecord, ExplanationView,
    ReasoningStep,
};
