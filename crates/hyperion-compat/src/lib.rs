//! Hyperion L1 Compatibility Layer — Phase 9, fourth and final slice.
//!
//! Implements docs/27-compatibility-layer.md's Trust Boundary isolation
//! and staged artifact-promotion gate — the Phase 9 exit criterion: "a
//! legacy Windows/Linux/Android application runs inside \[27\]'s Trust
//! Boundary without corrupting the Knowledge Graph." No real Windows/
//! Linux/Android binary executes in this hosted simulator (see this
//! crate's doc comment for what's stubbed); what's real is everything
//! docs/27 itself says doesn't depend on actually executing foreign
//! code: the Trust Boundary/grant bookkeeping, default-deny path
//! resolution, and the two-stage capture/promotion gate.
//!
//! Real: [`host::CompatHost::launch`] mints a fresh
//! `hyperion_capability::TrustBoundaryId` per session at
//! `max(profile.min_depth, target.default_depth())`, and — the literal
//! docs/27 §5 mechanism — resolves a Web-target `NetworkPolicy::Allow`
//! at admission time into a real `hyperion-netstack` domain-egress grant
//! scoped to the same domain pattern, "not a second, unrelated network-
//! access path." [`host::CompatHost::shim_open`] is docs/27 §3's
//! default-deny path gate: a `guest_path` outside every declared
//! filesystem root is refused outright, and a write additionally
//! requires an explicit write grant already present on the session —
//! the guest itself never holds a capability token, "every token
//! belongs to the Compatibility Host mediating on its behalf"
//! (confused-deputy prevention, the same invariant docs/03 states for
//! every other sandboxed subject in this workspace). The critical
//! separation the Knowledge-Graph-non-corruption guarantee rests on:
//! [`host::CompatHost::shim_open`]'s capture (Stage A) never writes to
//! the Knowledge Graph — only [`host::CompatHost::promote_artifact`]'s
//! explicit, consent-gated Stage B does, via the real
//! `hyperion-knowledge-graph::put_node`. [`host::CompatHost::terminate`]
//! implements the doc's "microreboot" recovery — cascade-revoking every
//! token the session was ever granted.
//! [`workspace_bridge::present_as_workspace`] implements docs/27's
//! "Window-to-Workspace binding" and "Accessibility bridging (bounded
//! exception)" for real: it wraps a session as the sole content of a
//! Workspace compiled through the real `hyperion-workspace` Phase 5
//! pipeline, binds the session's real promoted artifacts to that panel's
//! Context Bundle entries, and emits docs/27's literal "Limited
//! accessibility: legacy application" disclosure node whenever the
//! session's `accessibility_bridge` tier is not `Platform` — closing both
//! this crate's and `hyperion-workspace`'s own "no legacy-application
//! Workspace type exists yet" gap.
//!
//! ~~**Real Linux container/namespace runtime.**~~ — now real:
//! [`host::CompatHost::exec_in_sandbox`] spawns a genuine child process under real, kernel-
//! enforced PID/UTS/IPC (and, per `NetworkPolicy`, network) namespace isolation via `bwrap`
//! (bubblewrap) — see [`sandbox`]'s own doc comment for exactly what's kernel-enforced versus
//! honestly narrowed (there is no separate guest root filesystem image; a sandboxed guest runs
//! against this host's own base userland, confined to writing only the session's own declared
//! `filesystem_roots`).
//!
//! ~~**A real browser rendering engine** for `LegacyTarget::Web`~~ — now real:
//! [`host::CompatHost::render_web_page`] hands an already-`web_fetch`-authorized URL to a real,
//! already-installed headless Chromium-family binary (via [`browser::render_dom`]) and returns its
//! actual post-load, script-evaluated DOM — see that module's own doc comment for why this is a
//! second, independent real fetch rather than a reuse of `web_fetch`'s own bytes.
//!
//! Still deliberately deferred, and why (these two specifically require infrastructure this
//! hosted simulator was verified, not merely assumed, not to have — `/dev/kvm` exists as a device
//! node but this environment's own user is not in the `kvm` group and no hypervisor/VM-image
//! tooling (`qemu-system-x86_64`) is installed; no Android emulator, system image, or container
//! runtime (`waydroid`/`anbox`) is present either, and `adb` alone is a debug-bridge *client*, not
//! a container or ART runtime):
//!
//! - **Real Windows VM/hardware virtualization** (EPT/NPT, foreign guest
//!   kernel, virtual GPU output) — docs/27 assumes full VM, not a Wine-
//!   style API translation layer; nothing simulates a foreign kernel
//!   here.
//! - **Real Android container + translated permission surface + ART
//!   runtime.**
//! - **Real framebuffer/compositor capture and platform accessibility
//!   bridges** (Windows UI Automation, Android `AccessibilityService`,
//!   X11 AT-SPI, OCR-based pixel fallback). [`types::AccessibilityBridgeTier`]
//!   records *which* tier is active and
//!   [`workspace_bridge::present_as_workspace`] surfaces the required
//!   disclosure for it, but nothing here actually runs a platform
//!   accessibility API bridge or an OCR pass — a caller sets the tier
//!   directly, matching this crate's `sniffed_type`-as-caller-supplied
//!   precedent below.
//! - **Real content-type sniffing.** [`host::CompatHost::promote_artifact`]
//!   takes `sniffed_type` as a caller-supplied string — no real file-
//!   format detection runs; a caller (or a future integration with
//!   [22 — Local AI Runtime](../22-local-ai-runtime.md)) supplies it.
//! - **`ShimPathMapping.semantic_root`/case-insensitive path-translation
//!   details.** [`types::CompatibilityProfile::filesystem_roots`] is a
//!   flat list of guest-path-prefix strings; the doc's richer
//!   `ShimPathMapping` (per-root `semantic_root`/`case_sensitivity`) is
//!   narrowed to what the default-deny prefix check actually needs.
//! - **A clipboard/inter-Workspace IPC bridge.** Docs/27 itself gives no
//!   API signature for this ("described only prose-level... no API
//!   surfaced") — nothing to implement against.
//! - **A dedicated `TrustDepth` shared with `hyperion-plugin-framework`.**
//!   Each crate declares its own four-value depth label rather than
//!   sharing one — see [`types::TrustDepth`]'s own doc comment.

mod browser;
mod host;
mod sandbox;
mod types;
mod workspace_bridge;

pub use host::CompatHost;
pub use types::{
    AccessibilityBridgeTier, CompatError, CompatSession, CompatibilityProfile, IngestedArtifact,
    LegacyTarget, NetworkPolicy, PromotionPolicy, PromotionState, RenderedPage, SandboxExecution,
    SessionId, TrustDepth,
};
pub use workspace_bridge::present_as_workspace;
