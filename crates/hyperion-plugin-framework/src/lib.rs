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
//! outright.
//!
//! Deliberately deferred, and why:
//!
//! - **Eight of `Contribution`'s nine variants** (`Agent`, `Model`,
//!   `HardwareSupport`, `KnowledgeProvider`, `UiComponent`,
//!   `ExecutionEngine`, `AutomationWorkflow`, `MemoryProvider`) — only
//!   `Capability` has an owning subsystem in this workspace with a
//!   registration point to call; the other eight would register into
//!   crates (a device driver registry, a memory-provider registry) this
//!   workspace's Phase 1-8 crates don't expose an equivalent hook for.
//! - **Real publisher-key signature verification.** [`review::signature`]
//!   is the same non-cryptographic-checksum stand-in this workspace uses
//!   throughout (`hyperion-ai-runtime::checksum`, `hyperion-security`'s
//!   model integrity check) — not real cryptography.
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
//! - **`TrustDepth` as a real isolation mechanism.** It is enforced here
//!   purely as a policy label (a manifest's declared minimum compared
//!   against the caller-supplied `available_depth`) — see
//!   [`types::TrustDepth`]'s own doc comment for why this workspace has
//!   no second, deeper sandboxing primitive beyond `hyperion-capability`'s
//!   Trust Boundary to model a real depth spectrum with.

mod registry;
mod review;
mod types;

pub use registry::PluginRegistry;
pub use review::{signature, validate_manifest};
pub use types::{
    CapabilityGrantRequest, CapabilityId, CapabilityManifest, Contribution,
    ImplementationDescriptor, ImplementationKind, InstallState, Operation, PluginError,
    PluginHandle, PluginId, PluginManifest, QuarantineReason, RegistryEntry, SemanticContract,
    SideEffect, TrustDepth,
};
