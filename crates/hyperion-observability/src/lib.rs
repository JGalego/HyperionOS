//! Hyperion L0-cross-cutting Observability & Telemetry — Phase 8, fifth
//! slice.
//!
//! Implements docs/34-observability-telemetry.md's fork at the source
//! into two distinct pipelines: a lossy, sampled, best-effort path for
//! metrics/logs/traces ([`telemetry::TelemetryCollector`]), and a never-
//! sampled, durability-first, tamper-evident audit ledger
//! ([`ledger::AuditLedger`]) for security-relevant events — grants,
//! revocations, [18 — Explainability & Trust](../18-explainability-and-trust.md)'s
//! `ExplanationRecord`s, and [23 — Multi-Model Orchestration](../23-multi-model-orchestration.md)'s
//! routing `Rationale`s, each embedded into `AuditLogEntry.payload`
//! as-is rather than redefined.
//!
//! Real: [`ledger::AuditLedger::append`] is the *only* write path into
//! the ledger, hash-chained with a real BLAKE3 hash (PRODUCTION_BOOT_PROMPT.md M9 — see
//! [`ledger`]'s own doc comment; `entry_hash = H(prev_hash || canonical(payload) || seq)`) so
//! [`ledger::AuditLedger::verify_chain`] can detect both a broken hash link and a `seq` gap — the
//! first corruption is reported at its exact `seq`, never silently repaired, per docs/34 §5;
//! [`telemetry::TelemetryCollector`] implements the metrics/spans/logs
//! side deliberately *without* a capability check (see its doc comment
//! for why this asymmetry with the audit path is intentional, not an
//! oversight); [`telemetry::TelemetryCollector::merge_remote_trace`]
//! reconstructs one real cross-device trace tree from two independent
//! collectors' spans/logs for the same [`types::TraceId`] — docs/21's
//! distributed trace merging, made real, and now with a real production
//! call site: `hyperion-federation`'s `FederationHub` holds one
//! `TelemetryCollector` per device and `FederationHub::migrate` calls
//! this at exactly the point docs/21 describes, pulling the source
//! device's recorded trace into the target's collector before the
//! source instance is torn down; [`telemetry::ewma`]/[`telemetry::derivative`] are docs/34
//! §2's real scheduler-feedback estimators; [`aggregate::build_aggregate`]
//! implements docs/34 §5's k-anonymity gate exactly — suppressed entirely
//! (never partial) below the cohort floor or without opt-in consent.
//! [`types::AuditPayload::ModelRouting`] closes `hyperion-model-router`'s
//! own "every `RoutingDecision` carries its full `Rationale` inline, but
//! there is no separate persisted lookup" gap — `hyperion-api-gateway`'s
//! `invoke_capability` now appends every real routing decision's
//! `Rationale` here, giving it a real, durable, queryable log for the
//! first time, even though lookup is still by `seq`/`target`
//! (the capability id), not a dedicated `invocation_id` index.
//!
//! Deliberately deferred, and why:
//!
//! - **Periodic Ed25519-signed Merkle anchors over the hash chain.**
//!   `AuditLogEntry` has no `signature` field; the hash chain's tamper-evidence is now real
//!   BLAKE3 (PRODUCTION_BOOT_PROMPT.md M9), which alone already satisfies that milestone's own
//!   exit criterion ("rejected by a real signature *or* hash-chain check") for this crate's
//!   piece of it. docs/34 §2's fuller design layers a periodic signed Merkle root over segments
//!   of the chain on top of that (`device_key.sign(merkle_root(segment))` every
//!   `ANCHOR_INTERVAL` entries, "hardware root of trust where available, a software key
//!   otherwise") — a real, separate, additive enhancement (Merkle-tree construction plus
//!   anchor-interval bookkeeping) this crate does not yet build, named here rather than silently
//!   implied done by the hash-chain fix above. [`hyperion_crypto::Keystore`] (this workspace's
//!   new real software keystore) is exactly what a future pass would sign anchors with.
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
//! - **A globally-unique cross-device span identity.**
//!   [`telemetry::TelemetryCollector::merge_remote_trace`] is now really
//!   invoked from `hyperion-federation`'s `FederationHub::migrate` (see
//!   this crate's "Real:" section above), but it remains a best-effort
//!   append, not a CRDT merge: a span id is only unique *within* the
//!   collector that minted it, so a merged trace can contain two spans
//!   sharing a `span_id` if they originated on different devices — giving
//!   every span a real, globally-unique identity across devices is a
//!   further, separate refinement this doesn't attempt.

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
