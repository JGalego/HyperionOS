//! Hyperion Developer SDK — Phase 9, second slice.
//!
//! Implements docs/25-sdk.md's local test harness and publish workflow
//! on top of the real `hyperion-plugin-framework` registry — the Phase 9
//! exit criterion: "a third-party developer (not the core team) builds,
//! tests locally, and publishes a Capability using only [25]'s public
//! tooling, and the Model Router correctly selects between it and a
//! first-party equivalent." This crate is that tooling's Rust shape;
//! docs/25 itself is written in a TypeScript-flavored CDL (Capability
//! Definition Language) pseudocode, which this crate ports to a plain
//! Rust [`types::Contract`]/[`types::Implementation`] pair rather than
//! treating the TypeScript as anything but illustrative.
//!
//! Real: [`harness::run_harness`] implements docs/25 §3's two-layer
//! golden-case check exactly — Layer 1 is an exact structural shape
//! check (any mismatch is a hard fail, `tolerance` never buys it back);
//! Layer 2, reached only if Layer 1 passes, is a content-distance-vs-
//! `tolerance.content` check — plus the cross-implementation equivalence
//! check the Model Router's candidate-interchangeability assumption
//! depends on: if two implementations of the same Contract disagree on
//! any golden case's pass/fail verdict, that case is flagged, not
//! silently ignored. [`publish::prepare_submission`] implements docs/25
//! §4's static-permission-analysis gate — an implementation that
//! statically observed a permission its Contract never declared fails
//! the build outright, before human review ever sees it — and routes to
//! [`types::ReviewStatus::PendingHumanReview`] for any declared sensitive
//! permission (`NetworkEgress`/`Write`), matching docs/24's own review-
//! gate categories. [`publish::publish`]/[`publish::to_plugin_manifest`]
//! compile a (Contract, Implementation) pair into a real
//! `hyperion_plugin_framework::PluginManifest` and install it through
//! the real registry — never a second, parallel installation path.
//! [`codegen::review_and_build`] closes docs/998-roadmap.md's own "tool
//! creation" gap for real: freshly generated Rust source is rejected
//! outright if it contains `unsafe`, then really compiled (`cargo build
//! --release`) and really linted (`cargo clippy -- -D warnings`) in a
//! throwaway scratch package — only a source that survives all three
//! becomes a real, runnable
//! `hyperion_plugin_framework::NativeBinaryDescriptor`, installable
//! through the exact same [`publish::publish`] path (and therefore the
//! exact same sandboxed execution path) as a hand-written `NativeBinary`
//! implementation. [`execution_engine::resolve_via_engine`] closes docs/24's own "execution
//! engines register runtimes usable by Capability implementations" gap: a plugin's own
//! `hyperion_plugin_framework::Contribution::ExecutionEngine` supplies a reusable launcher, and
//! this turns a caller's own script into a concrete `NativeBinaryDescriptor` by prepending that
//! launcher — installed and invoked through the exact same `ImplementationKind::NativeBinary`
//! path, never a second, parallel execution mechanism.
//!
//! Deliberately deferred, and why:
//!
//! - **The `hyperion` CLI itself** (`scaffold`/`emulate`/`test`/`golden
//!   record`/`lint --permissions`/`status <submissionId>`). This crate is
//!   the library surface those subcommands would call; no command-line
//!   binary is built here.
//! - **`mockKnowledgeGraph`/real semantic-embedding fixtures.**
//!   [`types::MockContextBundle`] is a plain, hand-authored fixture —
//!   deliberately *not* wired to the real `hyperion-knowledge-graph`
//!   crate's richer shape, matching docs/25's own "never live data"
//!   framing; a caller wanting a realistic seeded graph composes
//!   `hyperion-knowledge-graph` directly.
//! - **A real `--channel beta|stable` staged rollout and the
//!   `MARKETPLACE SUBMISSION SERVICE` network call.** [`publish::publish`]
//!   installs directly into a local `PluginRegistry`; no network
//!   publish/submission-status-polling exists in a hosted simulator with
//!   no real network.
//! - ~~**`PublishSubmission.package_hash` real content fingerprinting.**~~ — now real:
//!   [`publish::prepare_submission`] computes a real BLAKE3 hash (via `hyperion_crypto::hash`,
//!   truncated to this field's own `u64` width) over the submission's own canonical (Contract,
//!   Implementation) bytes, rather than always leaving it at `0`. Distinct from
//!   [`publish::to_plugin_manifest`]'s own real Ed25519 signature (`hyperion_plugin_framework::
//!   sign`, itself real since M9 — not the non-cryptographic checksum this bullet originally
//!   described, which was already stale by the time this note was updated): the signature
//!   authenticates the *publisher*'s identity; `package_hash` fingerprints the *content* itself,
//!   independent of who signed it or how it was scored. Real package-level code *signing* (a
//!   caller verifying `package_hash` against a trusted registry entry before install) remains
//!   separate, real follow-up work this bullet does not claim to close.
//! - **`Implementation.resourceProfile`.** Not modeled — no consumer
//!   (this crate's harness doesn't schedule real resource contention).
//! - **A real embedding model for `embeddingDistance`.** [`harness::run_harness`]'s
//!   content-distance check is a token-overlap heuristic, the same
//!   documented downgrade `hyperion-netstack`'s entity resolution already
//!   uses in this workspace.

mod codegen;
mod execution_engine;
mod harness;
mod publish;
mod types;

pub use codegen::{review_and_build, CodegenRejection, GeneratedSource};
pub use execution_engine::resolve_via_engine;
pub use harness::{run_harness, CapabilityImplementation};
pub use publish::{prepare_submission, publish, to_plugin_manifest};
pub use types::{
    CaseVerdict, Contract, GoldenCase, HarnessReport, Implementation, ImplementationReport,
    LatencyClass, MockContextBundle, PermissionRequest, PublishSubmission, ReviewStatus, Runtime,
    SdkError, Tolerance, TrustLevel,
};
