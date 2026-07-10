//! Hyperion L6 Dynamic UI Runtime + Accessibility — Phase 5.
//!
//! Implements docs/13-dynamic-ui-runtime.md's Workspace Compiler and
//! docs/14-accessibility.md's accessibility tree derivation as one crate,
//! for the reason docs/14 §1 states directly: "every Panel... carries a
//! semantic accessibility tree derived in the *same* compilation pass as
//! its visual layout — not a separately-authored, separately-maintained
//! parallel UI." Splitting compiler and accessibility into two crates
//! would force exactly the two-pass drift both documents say must be
//! structurally impossible.
//!
//! Per docs/41-implementation-phases.md's own Phase 5 scope note — "a
//! compiled UI tree + accessibility tree as structured data is sufficient;
//! no real rendering surface is expected in a hosted simulator" — this
//! crate compiles [`WorkspaceGraph`]/[`AccessibilityTree`] as structured
//! data. There are no pixels anywhere in this crate.
//!
//! Real: the full compile pipeline (capability-to-panel mapping, Adaptive-
//! Complexity variant selection, structural cache-key derivation and
//! template reuse, lifecycle transitions); the accessibility tree derived
//! in the same pass with the doc's own deterministic fallback ("never emit
//! a nameless node" — a Capability that omits accessibility metadata still
//! gets a valid, if generic, node); the full linter rule set from docs/14
//! §5.2/§7, gating template cache admission exactly as the doc specifies;
//! and three modality projections over one shared tree (screen-reader
//! linearization, voice-control grammar feeding micro-Intents, switch-scan
//! grouping) — proving "one Capability UI contract yields several
//! modalities for free" without needing five hand-built interfaces.
//! `hyperion-device`'s `tests/cross_device_workspace.rs` (dev-dependency
//! only, no coupling added here) proves `DeviceRegistry::find_render_surfaces`'s
//! real query genuinely decides which, and how many, real devices a
//! compiled [`compiler::WorkspaceGraph`] mounts onto — docs/20 §5.5's
//! Cross-Device Workspace Assembly's first, closable half.
//!
//! Deliberately deferred, and why:
//!
//! - **Real rendering/compositor and pixel layout solve** — see this
//!   crate's own scope note above. [`compiler::WorkspaceCompiler::mount`]
//!   transitions lifecycle state; it does not paint anything.
//! - **Live incremental re-render over a real Event System** ([31 — Event
//!   System](../31-event-system.md), not built) — this crate has no
//!   `LiveUpdateEvent` subscription; a caller can re-derive/re-bind by
//!   calling compile again, which is correct but not incremental.
//! - **Real-time translation, captioning, ADHD reduced-distraction mode,
//!   dyslexia typography/pacing.** All four need either a real local
//!   translation/speech-to-text backend
//!   ([22 — Local AI Runtime](../22-local-ai-runtime.md)'s real backend,
//!   not the mock one) or a real rendering surface to retarget fonts/
//!   spacing against — neither exists yet.
//! - **Eye-gaze projection** — needs a real [`InputDeviceProfile`] from
//!   [20 — Device Framework](../20-device-framework.md) (Phase 7, not
//!   built) to size dwell targets against; screen-reader, voice, and
//!   switch-scan projections need no such device input.
//! - **The Compatibility Layer's bounded accessibility exception**
//!   ([27 — Compatibility Layer](../27-compatibility-layer.md), Phase 9,
//!   not built) — there is no legacy-application Workspace type here to
//!   carry it.

mod accessibility;
mod compiler;
mod contracts;
mod modality;
mod types;

pub use accessibility::{lint_template, AccessibilityLintResult, Severity, Violation};
pub use compiler::{WorkspaceCompiler, WorkspaceError};
pub use contracts::{CapabilityUiContract, ComplexityTier, PanelVariant, RegionAffinity};
pub use modality::{project, Modality, ModalityInterface};
pub use types::{
    AccessibilityNode, AccessibilityTree, Binding, BindingMode, CompiledLayoutTemplate,
    LifecycleState, Panel, RenderState, WorkspaceGraph, WorkspaceIntentKey,
};
