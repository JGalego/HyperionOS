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
//! `capability_id` competes as one more implementation or is rejected
//! outright. [`registry::PluginRegistry::agent_contributions`]
//! (2026-07-16) is the real, live registration point for
//! `Contribution::Agent` — install/uninstall/quarantine all treat it exactly like a
//! `Capability` contribution (minted no separate tokens of its own; an `Agent` contribution can
//! only ever justify `Read`/`Execute` permissions, never `Write`/`NetworkEgress` — see
//! [`review::contract_requires`]'s sibling check in `review.rs`).
//!
//! Deliberately deferred, and why:
//!
//! - **Six of `Contribution`'s remaining seven non-`Capability`
//!   variants** (`HardwareSupport`, `KnowledgeProvider`,
//!   `UiComponent`, `ExecutionEngine`, `AutomationWorkflow`,
//!   `MemoryProvider`) — none of these has an owning subsystem in this
//!   workspace with a real registration point to call yet (a device
//!   driver registry, a memory-provider registry). `Agent` (2026-07-16) is no longer one of
//!   them: [`registry::PluginRegistry::agent_contributions`] is the real, live registration
//!   point `hyperion-coordination::catalog::default_manifests`'s own doc comment named as
//!   missing — a plugin's `Contribution::Agent` now really competes for task allocation
//!   alongside the built-in roster, not just a hardcoded, static list. **`Model` is not actually a
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
//!   non-cryptographic checksum a forger could reproduce. Still deferred: docs/24's own
//!   "verify against publisher's registered key" implies a multi-publisher trust store; no such
//!   registry exists anywhere in this workspace, so this verifies against one real, trusted
//!   device identity instead — see [`hyperion_crypto`]'s own doc comment on why that's a
//!   deliberate, named scope boundary, not an oversight.
//! - **The Consent/Permission-Diff UI.** [`registry::PluginRegistry::install`]
//!   takes a plain `consented: bool` — the same "caller supplies the
//!   confirmation, no real prompt UI" pattern `hyperion-device`'s
//!   `pair(..., confirmed: bool)` already established for Actuate-tier
//!   pairing.
//! - **Consent diffing on update** (docs/24 §5: "update shows only
//!   grants not in `existing_grants`") — this crate has no
//!   `plugin_update` distinct from `uninstall` + `install`; a caller
//!   wanting the diff-only UX composes those two calls itself.
//! - **`registry_query`'s semantic-embedding+threshold variant.**
//!   [`registry::PluginRegistry::query`] is exact-`capability_id` lookup
//!   only — the embedding variant needs `hyperion-knowledge-graph`'s
//!   vector index, which this crate does not depend on to stay decoupled
//!   from a specific Knowledge Graph instance.
//! - **`version_variant()` minting a distinct id for an incompatible
//!   collision.** A structurally incompatible `capability_id` collision
//!   is rejected with [`types::PluginError::CapabilityCollisionIncompatible`]
//!   rather than automatically minted a versioned variant id — the
//!   caller can retry under an explicitly different `capability_id`.
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
pub use review::{sign, validate_manifest};
pub use types::{
    AgentContribution, CapabilityGrantRequest, CapabilityId, CapabilityManifest, Contribution,
    ImplementationDescriptor, ImplementationKind, InstallState, NativeBinaryDescriptor, Operation,
    PluginError, PluginHandle, PluginId, PluginManifest, QuarantineReason, RegistryEntry,
    SemanticContract, SideEffect, TrustDepth,
};
