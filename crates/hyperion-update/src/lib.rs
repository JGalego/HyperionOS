//! Hyperion L2 Platform Services — the Update System, Phase 10 first
//! slice.
//!
//! Implements docs/32-update-system.md's staged-rollout pipeline and its
//! explicit relationship to [33 — Rollback & Recovery](../33-rollback-recovery.md):
//! "\[33\] is what actually makes an update reversible." This is the
//! Phase 10 exit criterion most directly buildable for real in a hosted
//! simulator — no real fleet, CDN, or bootloader exists, but the
//! orchestration logic (compatibility gating, health-gated staged
//! advancement, and recovery-point-backed rollback) is real, and this
//! crate is a genuine consumer of the already-real `hyperion-recovery`
//! crate rather than a parallel undo mechanism.
//!
//! Real: [`orchestrator::UpdateOrchestrator::apply_update`] is docs/32
//! §1's pipeline exactly — signature verify → compatibility check →
//! `hyperion_recovery::RecoveryService::recovery_point_create(Trigger::PreUpdate)`
//! → monotonic, health-gated advancement through
//! `manifest.rollout_policy.stages` (default `[1%, 10%, 50%, 100%]` per
//! [`types::RolloutPolicy::default_schedule`]) — never time-gated alone.
//! [`orchestrator::UpdateOrchestrator::update_rollback`] is the literal
//! `update_rollback(subject, to_version) -> RollbackReceipt // delegates
//! to 33` interface docs/32 names: it calls the real
//! `hyperion_recovery::RecoveryService::restore_to` to revert data, then
//! moves the active-version pointer back — callable both from
//! `apply_update`'s own health-breach path mid-rollout and post-hoc
//! against an already-rolled-out subject, per docs/32's own dual usage.
//! [`system_image::SystemImageController`] implements docs/32 §5's *one
//! documented exception* to the `restore_to` rule: "system image
//! rollback never calls `restore_to` at all" — it is a pure active-slot-
//! pointer flip with a real boot-attempt-counter, deliberately kept
//! entirely separate from `hyperion-recovery` for exactly the reason the
//! doc gives (the Storage Engine's stores aren't slot-scoped; the same
//! live data boots regardless of which image is active).
//! [`telemetry::cohort_health_from_telemetry`] closes this crate's own
//! named "caller supplies `CohortHealth` directly" gap for real, reading
//! the most recent matching samples off a real
//! `hyperion_observability::TelemetryCollector` rather than a caller
//! inventing the numbers.
//!
//! Deliberately deferred, and why:
//!
//! - **Real fleet cohort selection across a real device population.**
//!   [`orchestrator::UpdateOrchestrator::apply_update`] takes a
//!   `health_for_stage` closure a caller drives directly — no
//!   `select_cohort`/hash-bucket-by-device-id exists; there is no real
//!   device population in a hosted simulator to bucket.
//! - **Real signed package fetch/CDN distribution.** `UpdateManifest`
//!   carries no `package_ref`/content hash to fetch — this crate
//!   receives an already-in-hand manifest, never downloads anything.
//! - **Automatic per-stage polling wired into `apply_update` itself.**
//!   [`telemetry::cohort_health_from_telemetry`] can build a real
//!   [`types::CohortHealth`] from `hyperion-observability`'s real
//!   metrics, but `apply_update`'s `health_for_stage` callback is still
//!   caller-driven — this crate has no real fleet cohort selection (see
//!   the bullet above) to decide *which* metric name/tag corresponds to
//!   "this stage's cohort," so it cannot call the telemetry reader
//!   itself without inventing that scoping convention.
//! - **A real expand/contract migration DSL with a declared inverse.**
//!   [`types::UpdateManifest::touched_objects`] is the flattened input
//!   `hyperion-recovery`'s bounded, per-object snapshot needs — this
//!   crate has no separate migration-plan representation or "run the
//!   declared inverse" step; reverting is entirely `restore_to`'s job.
//! - ~~**Real signature.**~~ — now real (PRODUCTION_BOOT_PROMPT.md M9):
//!   [`orchestrator::UpdateOrchestrator::apply_update`] checks a real Ed25519 signature (via
//!   [`hyperion_crypto`]) over [`orchestrator::sign`]'s canonical bytes, not the `DefaultHasher`
//!   (SipHash) stand-in this crate used before — that stand-in was doubly non-cryptographic:
//!   neither a real signature *nor* even the same non-cryptographic-checksum convention
//!   (`hyperion-ai-runtime::checksum`'s FNV1a) this workspace otherwise used consistently. Real
//!   publisher PKI is still deferred, for the same reason named in `hyperion-plugin-framework`'s
//!   own doc comment: no multi-publisher trust store exists anywhere in this workspace, so this
//!   verifies against one real, trusted device identity instead.
//! - **Real PKI beyond one device identity, anti-rollback monotonic counters.** docs/32's own
//!   "signed monotonic version counter" anti-downgrade mechanism does not exist here at all, in
//!   any form — `apply_update`'s `compatibility_check` only compares `from_version` against the
//!   currently active version, which rejects an update built against a *stale* base but not a
//!   deliberate downgrade to an older, still-validly-signed manifest. A real fix needs a
//!   monotonic counter this crate has nowhere to persist yet (no keystore/state store concept for
//!   it) — named here rather than silently left implied by the signature fix above.
//! - **Real bootloader A/B hardware.** [`system_image::SystemImageController`]
//!   simulates the slot/boot-attempt state machine in-process; nothing
//!   here writes to a real partition table or invokes a real bootloader.

mod orchestrator;
mod system_image;
mod telemetry;
mod types;

pub use orchestrator::{sign, UpdateOrchestrator};
pub use system_image::SystemImageController;
pub use telemetry::cohort_health_from_telemetry;
pub use types::{
    CohortHealth, CohortStage, CompatibilityCheckResult, HealthThresholds, RollbackReceipt,
    RolloutPolicy, RolloutState, SystemImageSlot, SystemImageSlotName, UpdateError, UpdateManifest,
    UpdateSubject, Version,
};
