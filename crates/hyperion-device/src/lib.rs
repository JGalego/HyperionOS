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
//!
//! Deliberately deferred, and why:
//!
//! - **Re-syncing the Knowledge Graph node after registration.**
//!   `heartbeat`/`tick`/`pair` update the in-process `DeviceObject`
//!   (presence, `last_heartbeat`, grants) but don't call `put_node`
//!   again — `heartbeat`/`tick` in particular take no
//!   `CapabilityMonitor`/`CapabilityToken` at all (a device's own
//!   physical heartbeat isn't itself a capability-mediated action, and
//!   `tick` sweeps every device at once, with no single token that would
//!   authorize writing all of them), so wiring a write into either would
//!   need its own real design pass rather than a mechanical copy-paste of
//!   `register`'s pattern. The KG node is a real, queryable registration-
//!   time snapshot, not yet a live mirror.
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

mod manifest;
mod registry;
mod types;

pub use manifest::sign;
pub use registry::{DeviceError, DeviceRegistry};
pub use types::{
    CapabilityManifestEntry, DeviceObject, DeviceType, Direction, PairingRecord, PresenceState,
    SafetyClass, TrustTier,
};
