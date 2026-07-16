//! Hyperion Testing Strategy + Performance Benchmarks — Phase 10, third
//! and final slice.
//!
//! Implements docs/35-testing-strategy.md's `ReleaseGate.evaluate(build)`
//! aggregator and docs/36-performance-benchmarks.md's tier-stratified
//! regression `gate_check`, combined into one crate because both docs'
//! own architecture diagrams converge at the same point — a "Release
//! candidate" decision — and both are fundamentally the same kind of
//! artifact: a pure aggregation/decision function over already-computed
//! sub-results, not a thing that itself runs `cargo test` or measures
//! real hardware.
//!
//! Real: [`benchmark::evaluate_gate`] is docs/36 §2's algorithm exactly
//! — percent delta against a same-`(spec_id, hardware_profile)`-keyed
//! baseline, gated only if the delta breaches the configured threshold, with the
//! configured [`types::GateAction`] deciding
//! block/warn/quarantine-and-rerun; [`benchmark::evaluate_sigma_gate`] is docs/36 §1/§2's real
//! statistical-significance test — a real z-score against a real, computed rolling-window
//! mean/standard deviation, gated the same way; [`benchmark::BenchmarkRegistry`]'s
//! baseline lookup is structurally same-tier-only — "never cross-tier
//! compare" holds because there is no code path that could look up a
//! different profile's baseline. [`release::evaluate_release`] is
//! docs/35 §1's `ReleaseGate.evaluate`: a build passes only if every
//! sub-suite is non-blocking *and* the benchmark gate didn't return
//! `Blocked`. [`types::SuiteReport::is_blocking`] implements the doc's
//! threat-regression provenance distinction exactly — for
//! `SuiteKind::ThreatRegression`, only a failure whose provenance is
//! `previously_mitigated` blocks release; a `never_tested` gap is
//! tracked, never blocking, matching this workspace's real
//! `hyperion-threat-model` regression suite's own T1-T8 catalog shape.
//! [`release::record_release_decision`]/[`release::verify_completeness`]
//! implement docs/35's completeness invariant against the real,
//! tamper-evident `hyperion-observability::AuditLedger` — every decision
//! this crate produces should correspond to exactly one signed ledger
//! entry, and [`release::verify_completeness`] is the real check that a
//! given set of builds all have one. [`types::HardwareReleaseCriteria`]
//! (docs/998-roadmap.md M13) extends `evaluate_release` to also gate on this roadmap's own
//! real hardware/boot surface — image build reproducibility, both real reference platforms
//! (M11/M12) boot-tested, and a real staged update applied to (and rolled back from) a real
//! running booted system (`boot/scripts/update-rollback-test.sh`, docs/41 Phase 10's literal
//! exit criterion) — the same "caller supplies an already-computed real fact" shape every other
//! criterion here already uses, not something this crate re-derives itself.
//!
//! Deliberately deferred, and why:
//!
//! - **Actually running the five suites `SuiteReport` summarizes.** This
//!   crate consumes already-computed pass/fail/quarantine counts; it
//!   does not itself invoke `cargo test`, replay a golden-intent corpus,
//!   inject chaos faults, or lint accessibility — those are this
//!   workspace's other crates' own test suites (most concretely,
//!   `hyperion-threat-model`'s eight T1-T8 regression files for
//!   `SuiteKind::ThreatRegression`).
//! - ~~**Sigma-based statistical-significance regression testing.**~~ — now real:
//!   [`types::RegressionThreshold::Sigma`] gates on a real z-score against
//!   [`benchmark::BenchmarkRegistry`]'s own real, per-`(spec_id, hardware_profile)` rolling
//!   result window (`RegressionGate.baseline_window_builds` trailing real `p99_ms` values, docs/36
//!   §1's own `baseline_window: {builds: u32}`), computed via [`benchmark::evaluate_sigma_gate`].
//!   [`types::RegressionThreshold::Percent`] (this crate's original mechanism) is unaffected and
//!   still compares against a single caller-set [`types::BenchmarkBaseline`] point instead.
//! - **A real hardware matrix / `hardware_profile_detect()`.**
//!   [`types::HardwareProfileId`] is a bare string a caller supplies —
//!   no real SBC/laptop/workstation/enterprise silicon exists to detect.
//! - **A real bisection agent on gate failure.** Docs/36 §2's
//!   `bisection_agent.start` on any `block_release` verdict is not
//!   invoked here — this crate reports the blocked verdict; finding the
//!   offending commit is separate infrastructure.
//! - **Real CI/fleet benchmark execution and telemetry ingestion.**
//!   [`types::BenchmarkResult`] is always caller-supplied; nothing here
//!   runs a timer against real code paths or reads a real telemetry
//!   pipeline.

mod benchmark;
mod release;
mod types;

pub use benchmark::{evaluate_gate, evaluate_sigma_gate, BenchmarkRegistry};
pub use release::{evaluate_release, record_release_decision, verify_completeness};
pub use types::{
    BenchmarkBaseline, BenchmarkCategory, BenchmarkResult, BenchmarkSpec, BudgetTree, GateAction,
    GateOutcome, HardwareProfileId, HardwareReleaseCriteria, RegressionGate, RegressionThreshold,
    ReleaseDecision, ReleaseGateError, ReleaseGateReport, SuiteKind, SuiteReport, Verdict,
};
