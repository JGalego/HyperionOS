//! Hyperion L1 Device Framework — Phase 7, first slice.
//!
//! Implements docs/20-device-framework.md's uniform model for "everything
//! the user owns that can render, sense, or act": a capability-secured
//! [`DeviceRegistry`] where every device is inert until a [`PairingRecord`]
//! exists (§8's "no implicit trust"), pairing is genuinely tiered
//! (view/sense/actuate, each its own grant with its own expiry — a device
//! compromised for sensing cannot silently escalate to actuation), and the
//! transient-connectivity state machine (§5.6) is real and driven
//! explicitly by a caller-supplied clock tick rather than a real timer,
//! consistent with this workspace's hosted-simulator convention.
//!
//! Real: the full presence state machine (`Connected` -> `Degraded` ->
//! `Disconnected`, with grace-period-gated transitions and reconnect
//! reset); tiered pairing where an `Actuate`-tier grant requires an
//! explicit, separately-flagged confirmation step (§5.3's "deliberate
//! exception to Universal Usability"); manifest-contract validation at
//! invocation time (an undeclared or wrong-direction capability is denied,
//! never dispatched); and substitute-device handoff (§10's canonical
//! "car loses connectivity mid-navigation, hands off to phone" example).
//! [`DeviceRegistry::register`] also persists every `DeviceObject` as a
//! real `hyperion-knowledge-graph` node (doc §4: "a Semantic Object
//! subtype") — [`DeviceRegistry::kg_node_for`] resolves a `device_id` to
//! the real node it created. `tests/context_device_anchor.rs`
//! (dev-dependency-only) proves that real node composes as a real
//! `hyperion-context` anchor with no code change on either side — docs/06
//! §Architecture's "device/session state" signal collector, which
//! `hyperion-context`'s own doc named as blocked on exactly this.
//! [`hardware_support::known_capability_manifest`] (2026-07-16,
//! docs/998-roadmap.md's Resourceful pillar) is a real "device driver registry": a plugin's
//! `hyperion_plugin_framework::Contribution::HardwareSupport` entries are searched for a known
//! `(device_type, manufacturer, model)`, and a real caller uses the match (if any) as the
//! expected manifest instead of hand-authoring one with no reference — `register`'s own real
//! signature requirement is completely untouched by this; the device (or its driver) still has
//! to really sign whatever manifest is finally used.
//!
//! Deliberately deferred, and why:
//!
//! - ~~Re-syncing the Knowledge Graph node after registration~~ — now real:
//!   [`registry::DeviceRegistry::heartbeat`]/[`registry::DeviceRegistry::tick`]/
//!   [`registry::DeviceRegistry::pair`]/[`registry::DeviceRegistry::revoke`] all really call
//!   `put_node` again (via a new, shared `resync_kg_node` helper, so every one of them writes the
//!   identical metadata shape) — `heartbeat`/`tick`'s own missing `CapabilityMonitor`/
//!   `CapabilityToken` is the real design decision this bullet used to name as needed: both now
//!   take one real, caller-supplied token (`tick`'s one token authorizes its whole real
//!   multi-device sweep, matching `hyperion-federation::start_lease_heartbeat`'s own established
//!   "caller supplies the token a background/periodic action reuses" precedent, rather than this
//!   registry minting its own internal one). The KG node's own metadata gained a real `pairing`
//!   sibling field (the current `PairingRecord`, or `null`) alongside `DeviceObject`'s
//!   already-flattened fields — the "grants" half of this gap — so `pair`/`revoke` have something
//!   real to re-sync too, without changing the shape any existing query over `manufacturer`/
//!   `model`/etc. already relied on. The KG node is now a real, live mirror, not just a
//!   registration-time snapshot.
//! - **Real discovery protocols** (mDNS/BLE/Matter/cloud-relay, §5.1) — a
//!   device's `CapabilityManifest` is supplied directly to
//!   [`DeviceRegistry::register`] by the caller, standing in for whatever
//!   protocol actually advertised it. No real radio, no real transport.
//! - ~~**Signed-manifest verification**~~ (§8's device-impersonation
//!   defense) — now real: `hyperion-crypto` (Phase 8/M9) is exactly the
//!   key material this bullet named as missing.
//!   [`DeviceRegistry::register`] now requires a real Ed25519
//!   [`hyperion_crypto::Signature`] over the manifest's own fields,
//!   verified against a caller-supplied [`hyperion_crypto::VerifyingKey`]
//!   before anything is recorded — [`manifest::sign`] is what a caller
//!   producing a real manifest uses, the same real signing/verifying
//!   split `hyperion-plugin-framework`/`hyperion-update` already
//!   established. One real, trusted device identity per this workspace's
//!   single-device model, not a multi-publisher PKI docs/20 doesn't
//!   specify.
//! - **Per-surface Context Bundle field-splitting** (§5.5, §7's
//!   `handle_cross_device_workspace`'s other half) — `tests/cross_device_workspace.rs`
//!   (dev-dependency-only, no production coupling to `hyperion-workspace`
//!   added) now proves [`DeviceRegistry::find_render_surfaces`]'s real
//!   query genuinely decides which, and how many, real devices a
//!   compiled `hyperion-workspace` Workspace mounts onto. It does not
//!   decide *which Context Bundle fields* each surface gets — every
//!   eligible surface mounts the same compiled graph — since that needs
//!   a real per-surface layout algorithm neither doc's pseudocode fully
//!   specifies.

mod hardware_support;
mod manifest;
mod registry;
mod types;

pub use hardware_support::known_capability_manifest;
pub use manifest::sign;
pub use registry::{DeviceError, DeviceRegistry};
pub use types::{
    CapabilityManifestEntry, DeviceObject, DeviceType, Direction, PairingRecord, PresenceState,
    SafetyClass, TrustTier,
};
