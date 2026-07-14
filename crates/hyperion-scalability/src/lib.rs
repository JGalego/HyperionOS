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
//! [`fit::scheduler_has_headroom_for`]/[`fit::scheduler_backed_resolver`]
//! implement `scheduler.would_fit(...)` for real against
//! `hyperion_scheduler::Scheduler`'s live `Ram`/`Vram` ledgers — the two
//! of this crate's three `CapacityDescriptor` dimensions the scheduler's
//! `ResourceVector` actually models the same way (see [`fit`]'s doc
//! comment for why `compute_tops` is deliberately left unchecked rather
//! than forced onto a scheduler dimension that means something
//! different).
//!
//! Deliberately deferred, and why:
//!
//! - **Real hardware detection.** `hardware_profile_detect()` would read
//!   actual silicon/thermal sensors; this crate's [`types::HardwareProfile`]
//!   is always caller-supplied, matching this workspace's established
//!   `HardwareProfileSource`-as-fixture pattern.
//! - **Full nine-dimension parity with `hyperion_scheduler::ResourceVector`,
//!   and a `Substitution` -> resource-footprint mapping.**
//!   [`degrade::degrade_capability`]'s `resolve_alternate_fits` callback
//!   is still caller-supplied — [`fit::scheduler_backed_resolver`] now
//!   builds a real one backed by the scheduler's actual `Ram`/`Vram`
//!   ledgers, but this crate's own [`types::Substitution`] carries no
//!   `CapacityDescriptor` (a `ModelTier`/`CapabilityRef` alone doesn't
//!   say how much RAM/VRAM it needs), so the caller still supplies that
//!   lookup; and `compute_tops`/storage IOPS/network bandwidth/battery
//!   are still not checked — this crate's own narrow
//!   [`types::ResourceConstraint`] (RAM/VRAM/compute-TOPS, matching
//!   docs/37's own tier table) was a deliberate choice not to force
//!   parity with dimensions docs/37's tier discussion never touches, and
//!   `compute_tops` specifically has no scheduler dimension that means
//!   the same thing (see [`fit`]'s doc comment).
//! - **`capability_registry.install(plan.capability)`.** [`explain::apply_and_explain`]
//!   still doesn't install a substituted implementation — a full
//!   `CapabilityManifest` is genuinely more than a bare
//!   [`types::Substitution`] carries, and installation is
//!   `hyperion-plugin-framework`'s own capability-gated, signature-
//!   verified operation, not something to route around. What's real now:
//!   an `AlternateImplementation` substitution's target is confirmed
//!   against a caller-supplied real `PluginRegistry` (`registry.query(...)`
//!   already does exact-id lookup; a valid target must already be a
//!   registered, non-quarantined capability, nothing needed constructing)
//!   before the audit notice is written, refusing to claim a fallback
//!   happened against a capability that was never actually installed.
//!   `CheaperLocalTier`/`ConsentedCloudUpgrade` aren't pre-registered
//!   capabilities the same way, so this check doesn't cover them.
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
mod fit;
mod types;

pub use degrade::degrade_capability;
pub use explain::apply_and_explain;
pub use fit::{scheduler_backed_resolver, scheduler_has_headroom_for};
pub use types::{
    CapabilityRef, CapacityDescriptor, DegradationOutcome, DegradationPlan, DegradationPolicy,
    HardwareProfile, HardwareTier, ModelTier, ResourceConstraint, ScalabilityError, Substitution,
    TenancyMode,
};
