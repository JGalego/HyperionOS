//! Hyperion cross-cutting Scalability Roadmap — Phase 10, second slice.
//!
//! Implements docs/37-scalability-roadmap.md's `degrade_capability`
//! decision procedure — the one concrete, deterministic algorithm the
//! doc gives, real and directly portable to a hosted simulator with no
//! real silicon, fleet, or federation to model the rest of the doc
//! against.
//!
//! Real: [`degrade::degrade_capability`] is docs/37 §3's pseudocode
//! exactly — a policy whose `ResourceConstraint` the hardware satisfies
//! is full fidelity; otherwise the fixed fallback order (cheaper local
//! tier → alternate local implementation → consented cloud upgrade →
//! disable) is walked in sequence, never reordered per call, and
//! security policy is never a substitution target *by construction*
//! (`Substitution` has no variant that could touch
//! [15 — Security Architecture](../15-security-architecture.md)).
//! `ConsentedCloudUpgrade` checks `hyperion-privacy`'s real
//! `ConsentLedger` for a standing grant — never assuming consent, the
//! same invariant every other consent check in this workspace holds.
//! [`explain::apply_and_explain`] writes a real, tamper-evident
//! `hyperion-observability` audit entry, closing docs/37's own named
//! "explanation lag" failure mode (a degraded Capability running
//! silently before its notice catches up).
//!
//! Deliberately deferred, and why:
//!
//! - **Real hardware detection.** `hardware_profile_detect()` would read
//!   actual silicon/thermal sensors; this crate's [`types::HardwareProfile`]
//!   is always caller-supplied, matching this workspace's established
//!   `HardwareProfileSource`-as-fixture pattern.
//! - **`scheduler.would_fit(...)` against `hyperion_scheduler::ResourceVector`.**
//!   [`degrade::degrade_capability`]'s `resolve_alternate_fits` callback
//!   is a caller-supplied yes/no — this crate defines its own narrow
//!   [`types::ResourceConstraint`] (RAM/VRAM/compute-TOPS only, matching
//!   docs/37's own tier table) rather than forcing parity with the real
//!   scheduler's nine-dimension vector, which models several dimensions
//!   (storage IOPS, network bandwidth, battery) docs/37's tier
//!   discussion never touches.
//! - **`capability_registry.install(plan.capability)`.** [`explain::apply_and_explain`]
//!   writes the audit notice only; actually installing a substituted
//!   implementation through `hyperion-plugin-framework`'s registry needs
//!   a full `CapabilityManifest` a bare [`types::Substitution`] doesn't
//!   carry enough information to construct — deferred to whichever
//!   future integration wires degradation decisions into real plugin
//!   installation.
//! - **KG partitioning / `TenantPartition` / cross-tenant edges.**
//!   [`types::TenancyMode::MultiTenantOrg`] is declared as a hardware-
//!   tier-unlocked mode; no partitioning logic exists here — `hyperion-
//!   knowledge-graph` has no shard concept to partition.
//! - **`FederationMembership`/`federation_join`/`federation_revoke`.**
//!   `hyperion-federation` already owns real Trust-Boundary-per-device
//!   membership; this crate does not duplicate it.
//! - **Watchdog-triggered proactive re-degradation, KG hot-spot
//!   splitting, autoscale queuing.** All of docs/37 §5's recovery
//!   mechanisms need a live fleet/cluster this hosted simulator has no
//!   equivalent for.

mod degrade;
mod explain;
mod types;

pub use degrade::degrade_capability;
pub use explain::apply_and_explain;
pub use types::{
    CapabilityRef, CapacityDescriptor, DegradationOutcome, DegradationPlan, DegradationPolicy,
    HardwareProfile, HardwareTier, ModelTier, ResourceConstraint, ScalabilityError, Substitution,
    TenancyMode,
};
