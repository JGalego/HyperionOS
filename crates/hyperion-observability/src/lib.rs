//! Hyperion L0-cross-cutting Observability & Telemetry — Phase 8, fifth
//! slice.
//!
//! Implements docs/34-observability-telemetry.md's fork at the source
//! into two distinct pipelines: a lossy, sampled, best-effort path for
//! metrics/logs/traces ([`telemetry::TelemetryCollector`]), and a never-
//! sampled, durability-first, tamper-evident audit ledger
//! ([`ledger::AuditLedger`]) for security-relevant events — grants,
//! revocations, and [18 — Explainability & Trust](../18-explainability-and-trust.md)'s
//! `ExplanationRecord`s, embedded into `AuditLogEntry.payload` as-is
//! rather than redefined.
//!
//! Real: [`ledger::AuditLedger::append`] is the *only* write path into
//! the ledger, hash-chained (`entry_hash = H(prev_hash || canonical(payload)
//! || seq)`) so [`ledger::AuditLedger::verify_chain`] can detect both a
//! broken hash link and a `seq` gap — the first corruption is reported at
//! its exact `seq`, never silently repaired, per docs/34 §5;
//! [`telemetry::TelemetryCollector`] implements the metrics/spans/logs
//! side deliberately *without* a capability check (see its doc comment
//! for why this asymmetry with the audit path is intentional, not an
//! oversight); [`telemetry::ewma`]/[`telemetry::derivative`] are docs/34
//! §2's real scheduler-feedback estimators; [`aggregate::build_aggregate`]
//! implements docs/34 §5's k-anonymity gate exactly — suppressed entirely
//! (never partial) below the cohort floor or without opt-in consent.
//!
//! Deliberately deferred, and why:
//!
//! - **Hardware root-of-trust / TPM signing of Merkle anchors.**
//!   `AuditLogEntry` has no `signature` field; the hash chain's tamper-
//!   evidence is real, its cryptographic anchoring to hardware is not —
//!   docs/34 itself says this degrades gracefully to a software key,
//!   which this crate doesn't model as a distinct code path since no
//!   crate in this workspace has a signing key concept yet.
//! - **The Fleet aggregate-submission network endpoint**
//!   (`Fleet.submitAggregate`). [`aggregate::build_aggregate`] produces
//!   the gated report; nothing here sends it anywhere — no real network
//!   transport exists in this hosted simulator.
//! - **`Scheduler.subscribeLoadSignal` wiring.** [`telemetry::ewma`]/
//!   [`telemetry::derivative`] are the real estimators docs/34 §2
//!   describes feeding a `LoadSignal`; this crate does not itself
//!   publish to `hyperion-scheduler`, which has no subscription API to
//!   receive one.
//! - **Retention/rollup compaction of metrics and logs.** Samples and
//!   log events accumulate for the process lifetime; docs/34 §5's 24h-
//!   then-percentile-rollup aging is not implemented (mirrors
//!   `hyperion-recovery`'s equivalent retention deferral).
//! - **Ring-buffer write-ahead spill on store degradation, and
//!   background scheduled chain verification.**
//!   [`ledger::AuditLedger::verify_chain`] is on-demand only, not run on
//!   a background schedule.
//! - **Real distributed trace merging across devices** ([21 —
//!   Distributed Execution](../21-distributed-execution.md)) — `trace_id`
//!   here is a single-process concept; `hyperion-federation`'s per-device
//!   `AgentRuntime` instances are not wired to share one trace.

mod aggregate;
mod ledger;
mod telemetry;
mod types;

pub use aggregate::build_aggregate;
pub use ledger::AuditLedger;
pub use telemetry::{derivative, ewma, TelemetryCollector};
pub use types::{
    AggregateReport, AuditAction, AuditLogEntry, AuditPayload, ConsentCategory, ConsentScope,
    LogEvent, LogLevel, MetricSample, ObservabilityError, PrincipalRef, RedactionClass, SpanStatus,
    TraceId, TraceSpan, VerificationReport,
};
