//! Hyperion L2 Platform Services — Plugin Framework, Phase 9 first slice.
//!
//! Implements docs/24-plugin-framework.md's manifest review gate,
//! capability-token minting on install, and the shared Capability
//! Registry [23 — Multi-Model Orchestration](../23-multi-model-orchestration.md)'s
//! Model Router selects candidates from — the Phase 9 entry criterion
//! ("a Plugin ecosystem opened before the capability-security model is
//! hardened is a threat-model violation") is satisfied by this crate
//! doing nothing but calling into the already-hardened
//! `hyperion-capability`/Phase 8 crates, never inventing a second
//! permission system.
//!
//! Real: [`review::validate_manifest`] implements docs/24 §5's review-
//! gate over-request check exactly — a requested permission is rejected
//! pre-consent unless it's justified by a declared `SideEffect`
//! somewhere in the manifest, never surfaced as a choice the user could
//! accidentally approve; [`registry::PluginRegistry::install`] mints
//! *exactly* the requested tokens (never a superset) under a fresh Trust
//! Boundary via `hyperion-capability`'s real `cap_derive`;
//! [`registry::PluginRegistry::uninstall`] revokes every one of them via
//! real cascade `cap_revoke` — "one graph walk invalidates everything,"
//! reusing the same generation-based revocation every other crate in
//! this workspace already depends on, not a second one;
//! [`registry::PluginRegistry::register_implementation`] implements the
//! structural-compatibility check that decides whether a colliding
//! `capability_id` competes as one more implementation, or — via a real
//! [`registry::version_variant`]-minted id — registers as a genuinely
//! separate entry instead.
//!
//! Every non-`Capability` `Contribution` variant this crate implements now has a real, live
//! registration point (all landed 2026-07-16, docs/998-roadmap.md's Resourceful pillar) — each
//! `Read`-only (or `Read`/`Execute` where the contribution's own existence implies dispatch),
//! and each producing the exact struct a hand-authored equivalent already would, never a second,
//! parallel dispatch/compilation path:
//!
//! - `Agent`: [`registry::PluginRegistry::agent_contributions`] — a plugin's specialization now
//!   really competes for task allocation alongside
//!   `hyperion-coordination::catalog::default_manifests`'s built-in roster.
//! - `HardwareSupport`: [`registry::PluginRegistry::hardware_support_contributions`] — a plugin
//!   teaches Hyperion the expected capability manifest for a known `(manufacturer, model)`,
//!   without weakening `hyperion_device::DeviceRegistry::register`'s own real signature check at
//!   all.
//! - `KnowledgeProvider`: [`registry::PluginRegistry::knowledge_provider_contributions`] — a real
//!   (topic -> capability_id) lookup `hyperion-knowledge-graph` had no equivalent of.
//! - `UiComponent`: [`registry::PluginRegistry::ui_component_contributions`] — a real registry
//!   `hyperion-workspace` had no equivalent of for a `CapabilityUiContract`.
//! - `AutomationWorkflow`: [`registry::PluginRegistry::automation_workflow_contributions`] — a
//!   plugin's goal template now really competes for a real utterance match alongside
//!   `hyperion-intent`'s own hardcoded, crate-private `TEMPLATES`.
//! - `MemoryProvider`: [`registry::PluginRegistry::memory_provider_contributions`] — a real
//!   `(tier, entity_key) -> capability_id` lookup `hyperion-memory` had no equivalent of, the
//!   same honest, never-bypass-dispatch shape `KnowledgeProvider` already established.
//! - `ExecutionEngine`: [`registry::PluginRegistry::execution_engine`] — a real, reusable
//!   launcher other Capability implementations can run their own script through
//!   (`hyperion_sdk::resolve_via_engine` turns a caller's script into a concrete
//!   `NativeBinaryDescriptor` by prepending it), instead of each one shipping a whole standalone
//!   native binary. The launcher itself is validated the exact same honest way a `Capability`'s
//!   own `NativeBinaryDescriptor` already is (must really exist, must really be executable).
//!
//! `install`/`uninstall`/`quarantine` all treat every one of these exactly like a `Capability`
//! contribution (minting no separate tokens of their own beyond `ExecutionEngine`'s sandbox-free
//! validation check).
//!
//! Deliberately deferred, and why:
//!
//! - **`Model` is not actually a ninth gap**, on inspection: a "this implementation is backed by a
//!   model" contribution is already exactly what `Contribution::Capability`'s
//!   `CapabilityManifest.implementation_kind` (`LocalSmallModel`/
//!   `LocalLargeModel`) expresses, and `hyperion-api-gateway`'s
//!   `router_bridge` already bridges every installed `Capability`
//!   contribution — model-backed or not — into a real
//!   `hyperion-model-router::register_implementation` call. Adding a
//!   separate `Contribution::Model` variant would duplicate that shape,
//!   not close a real gap.
//! - ~~**Real publisher-key signature verification.**~~ — now real
//!   (docs/998-roadmap.md M9): [`registry::PluginRegistry::install`] checks a real Ed25519
//!   signature (via [`hyperion_crypto`]) over [`review::sign`]'s canonical bytes, not a
//!   non-cryptographic checksum a forger could reproduce.
//! - ~~**Multi-publisher trust store.**~~ (2026-07-16) — now real: docs/24's own "verify against
//!   publisher's registered key" is exactly [`hyperion_crypto::PublisherRegistry`], and
//!   [`registry::PluginRegistry::install_with_publisher_registry`]/`update_with_publisher_registry`
//!   resolve a manifest's real trusted key from its own declared `publisher` instead of taking one
//!   caller-supplied key on faith. [`registry::PluginRegistry::install`]/`update` are unchanged —
//!   still verify against one caller-supplied key directly, every existing caller's default; the
//!   registry-aware entry points are additive. An unregistered publisher is a real, honest
//!   [`types::PluginError::UnknownPublisher`], never a silent fall-through to some other trust.
//! - **The Consent/Permission-Diff UI.** [`registry::PluginRegistry::install`]
//!   takes a plain `consented: bool` — the same "caller supplies the
//!   confirmation, no real prompt UI" pattern `hyperion-device`'s
//!   `pair(..., confirmed: bool)` already established for Actuate-tier
//!   pairing.
//! - ~~**Consent diffing on update**~~ (docs/24 §5: "update shows only grants not in
//!   `existing_grants`") — now real: [`registry::PluginRegistry::update`] compares
//!   `new_manifest.requested_permissions` against the plugin's currently-installed set (by
//!   `(operation, scope)`, ignoring `justification` wording) and returns exactly the new grants a
//!   real consent UI should present — empty, and no consent required at all, when the update adds
//!   nothing. A grant unchanged across the update reuses its exact original token rather than
//!   being re-minted from scratch; a grant the new manifest drops is really revoked. Every
//!   contribution is re-registered from the new manifest (the old ones are removed first, reusing
//!   [`registry::PluginRegistry::uninstall`]'s own non-token cleanup) — an update really replaces
//!   what a plugin contributes, it doesn't merely top up its permissions.
//! - **`registry_query`'s semantic-embedding+threshold variant.**
//!   [`registry::PluginRegistry::query`] is exact-`capability_id` lookup
//!   only — the embedding variant needs `hyperion-knowledge-graph`'s
//!   vector index, which this crate does not depend on to stay decoupled
//!   from a specific Knowledge Graph instance.
//! - ~~**`version_variant()` minting a distinct id for an incompatible collision.**~~ — now real:
//!   [`registry::PluginRegistry::register_implementation`] mints a real, deterministic
//!   `capability_id#N` variant id for a structurally incompatible collision instead of rejecting
//!   the whole install with [`types::PluginError::CapabilityCollisionIncompatible`] — docs/24
//!   §5's own pseudocode, so an incompatible manifest now installs in full and competes under its
//!   own distinct registry entry rather than failing outright.
//! - **`TrustDepth` as a real isolation mechanism.** Still enforced here purely as a policy label
//!   (a manifest's declared minimum compared against the caller-supplied `available_depth`) — but
//!   the premise that this workspace has no second, deeper sandboxing primitive is now stale:
//!   `hyperion-trust-boundary` (real Linux user namespaces, Landlock, and seccomp-bpf via
//!   `spawn`/`SpawnedBoundary::revoke`, live-tested against real forked processes) is exactly
//!   that. What's still missing isn't the primitive, it's something for it to attach *to*: this
//!   registry stores `ImplementationDescriptor`s (data), never a callable — `invoke_capability`'s
//!   real dispatch always goes through `hyperion-agent-runtime`'s stub-dispatch fallback (see
//!   `hyperion-api-gateway`'s own identical gap), because no out-of-process Capability execution
//!   exists anywhere in this workspace yet. Wiring `TrustDepth` onto `hyperion-trust-boundary` for
//!   real needs that out-of-process execution model built first — a real, separate feature, not a
//!   dependency swap in this crate.

mod registry;
mod review;
mod types;

pub use registry::PluginRegistry;
pub use review::{sign, validate_manifest, validate_manifest_against_registry};
pub use types::{
    AgentContribution, AutomationWorkflowContribution, CapabilityGrantRequest, CapabilityId,
    CapabilityManifest, Contribution, ExecutionEngineContribution, HardwareCapabilityEntry,
    HardwareDeviceType, HardwareDirection, HardwareSafetyClass, HardwareSupportContribution,
    ImplementationDescriptor, ImplementationKind, InstallState, KnowledgeProviderContribution,
    MemoryProviderContribution, MemoryTierKind, NativeBinaryDescriptor, Operation, PluginError,
    PluginHandle, PluginId, PluginManifest, QuarantineReason, RegistryEntry, SemanticContract,
    SideEffect, TrustDepth, UiComponentContribution, UiRegionAffinity, WorkflowLeaf,
};
