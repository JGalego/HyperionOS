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
//! the ledger, hash-chained with a real BLAKE3 hash (docs/998-roadmap.md M9 — see
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
//! - ~~**Periodic Ed25519-signed Merkle anchors over the hash chain.**~~ Now real:
//!   [`ledger::AuditLedger::new_with_keystore`] opts a ledger into producing a real, signed
//!   [`ledger::Anchor`] every `ANCHOR_INTERVAL` entries — `device_key.sign(merkle_root(segment))`,
//!   exactly docs/34 §2's fuller design, layered additively on top of the hash chain (the plain
//!   [`ledger::AuditLedger::new`] behaves exactly as before: no anchors, same as when this bullet
//!   was written). [`ledger::AuditLedger::verify_anchor`] checks both that an anchor's own
//!   signature verifies and that its claimed root still matches the ledger's current entries —
//!   catching a wholesale-rewritten segment a bare hash-chain re-verification, done by a party
//!   that never watched the chain grow in real time, couldn't distinguish from a legitimate one
//!   signed later. Hardware-backed anchoring ("hardware root of trust where available") remains
//!   real-hardware-only, same as this workspace's other TPM-adjacent deferrals.
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
pub use ledger::{Anchor, AuditLedger};
pub use telemetry::{derivative, ewma, TelemetryCollector};
pub use types::{
    AggregateReport, AuditAction, AuditLogEntry, AuditPayload, ConsentCategory, ConsentScope,
    LogEvent, LogLevel, MetricSample, ObservabilityError, PrincipalRef, RedactionClass, SpanStatus,
    TraceId, TraceSpan, VerificationReport,
};
