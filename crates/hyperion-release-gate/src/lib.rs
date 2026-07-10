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
//! baseline, gated only if the delta breaches `threshold_pct`, with the
//! configured [`types::GateAction`] deciding
//! block/warn/quarantine-and-rerun; [`benchmark::BenchmarkRegistry`]'s
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
//! given set of builds all have one.
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
//! - **Sigma-based statistical-significance regression testing.**
//!   [`types::RegressionGate`] is a flat percentage threshold; docs/36's
//!   `{sigma: f32}` variant needs a sample-variance history this crate
//!   doesn't maintain.
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

pub use benchmark::{evaluate_gate, BenchmarkRegistry};
pub use release::{evaluate_release, record_release_decision, verify_completeness};
pub use types::{
    BenchmarkBaseline, BenchmarkCategory, BenchmarkResult, BenchmarkSpec, BudgetTree, GateAction,
    GateOutcome, HardwareProfileId, RegressionGate, ReleaseDecision, ReleaseGateError,
    ReleaseGateReport, SuiteKind, SuiteReport, Verdict,
};
