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
//! - **Real `hyperion-observability` telemetry polling for
//!   `cohort_health`.** A caller supplies `CohortHealth` per stage
//!   directly, exactly as it would read one off a real dashboard —
//!   wiring this to `hyperion-observability`'s real metrics is future
//!   integration work, not attempted here.
//! - **A real expand/contract migration DSL with a declared inverse.**
//!   [`types::UpdateManifest::touched_objects`] is the flattened input
//!   `hyperion-recovery`'s bounded, per-object snapshot needs — this
//!   crate has no separate migration-plan representation or "run the
//!   declared inverse" step; reverting is entirely `restore_to`'s job.
//! - **Real signature/PKI, anti-rollback monotonic counters.**
//!   [`orchestrator::signature`] is the same non-cryptographic-checksum
//!   stand-in this workspace uses throughout.
//! - **Real bootloader A/B hardware.** [`system_image::SystemImageController`]
//!   simulates the slot/boot-attempt state machine in-process; nothing
//!   here writes to a real partition table or invokes a real bootloader.

mod orchestrator;
mod system_image;
mod types;

pub use orchestrator::{signature, UpdateOrchestrator};
pub use system_image::SystemImageController;
pub use types::{
    CohortHealth, CohortStage, CompatibilityCheckResult, HealthThresholds, RollbackReceipt,
    RolloutPolicy, RolloutState, SystemImageSlot, SystemImageSlotName, UpdateError, UpdateManifest,
    UpdateSubject, Version,
};
