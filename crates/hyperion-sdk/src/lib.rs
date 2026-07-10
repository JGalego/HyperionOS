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
//! - **Real code signing.** `PublishSubmission.package_hash` is left at
//!   `0` by [`publish::prepare_submission`] — this crate reuses
//!   `hyperion_plugin_framework::signature`'s non-cryptographic-checksum
//!   pattern only at the manifest level (via
//!   [`publish::to_plugin_manifest`]), not as a separate package-level
//!   signing step.
//! - **`Implementation.resourceProfile`.** Not modeled — no consumer
//!   (this crate's harness doesn't schedule real resource contention).
//! - **A real embedding model for `embeddingDistance`.** [`harness::run_harness`]'s
//!   content-distance check is a token-overlap heuristic, the same
//!   documented downgrade `hyperion-netstack`'s entity resolution already
//!   uses in this workspace.

mod harness;
mod publish;
mod types;

pub use harness::{run_harness, CapabilityImplementation};
pub use publish::{prepare_submission, publish, to_plugin_manifest};
pub use types::{
    CaseVerdict, Contract, GoldenCase, HarnessReport, Implementation, ImplementationReport,
    LatencyClass, MockContextBundle, PermissionRequest, PublishSubmission, ReviewStatus, Runtime,
    SdkError, Tolerance, TrustLevel,
};
