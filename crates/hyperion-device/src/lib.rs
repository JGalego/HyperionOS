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
//!
//! Deliberately deferred, and why:
//!
//! - **Persisting `DeviceObject` as a real Knowledge Graph node.** Doc
//!   §4 calls it "a Semantic Object subtype," but this slice keeps the
//!   registry in-process rather than wiring `hyperion-knowledge-graph` —
//!   the pairing/trust/presence state machine is the architectural core
//!   this phase needs proven; persistence is a mechanical follow-on with
//!   no new algorithm of its own, the same judgment call
//!   `hyperion-model-router` made about a real Capability Registry.
//! - **Real discovery protocols** (mDNS/BLE/Matter/cloud-relay, §5.1) — a
//!   device's `CapabilityManifest` is supplied directly to
//!   [`DeviceRegistry::register`] by the caller, standing in for whatever
//!   protocol actually advertised it. No real radio, no real transport.
//! - **Signed-manifest verification** (§8's device-impersonation defense)
//!   — manifests are trusted as given; real signature verification needs
//!   [15 — Security Architecture](../15-security-architecture.md) (Phase
//!   8) key material, the same dependency every other crate in this
//!   workspace defers real signing to.
//! - **Cross-device Workspace assembly** (§5.5, §7's `handle_cross_device_workspace`)
//!   — needs a real `hyperion-workspace` integration deciding which
//!   Context Bundle fields go to which surface; this crate exposes the
//!   registry query that step would consult ([`DeviceRegistry::find_render_surfaces`])
//!   but does not itself drive Workspace generation.

mod registry;
mod types;

pub use registry::{DeviceError, DeviceRegistry};
pub use types::{
    CapabilityManifestEntry, DeviceObject, DeviceType, Direction, PairingRecord, PresenceState,
    SafetyClass, TrustTier,
};
