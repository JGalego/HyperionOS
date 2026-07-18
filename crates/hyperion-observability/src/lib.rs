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
//! first time. ~~Lookup was still by `seq`/`target` (the capability id), not a dedicated
//! `invocation_id` index~~ (2026-07-16) — now real: `ModelRouting` carries its own real
//! `invocation_id`, and [`ledger::AuditLedger::rationale_for_invocation`] is docs/23's own
//! literal, previously-unbuilt `get_rationale(decision_id) -> Rationale`.
//!
//! [`telemetry::TelemetryCollector::compact_metrics`]/[`telemetry::TelemetryCollector::expire_logs`]
//! (2026-07-16) close this crate's own previously-named "retention/rollup compaction" gap:
//! docs/34 §5's "raw metrics are kept at full resolution for a short window (default 24h) then
//! compacted to percentile rollups" is now real — every real raw [`types::MetricSample`] older
//! than a caller-supplied retention window is removed from raw storage and folded into one real
//! [`types::MetricRollup`] per metric name (real min/max/count and real p50/p95/p99 via the
//! nearest-rank method, computed from the actual aged-out values, never fabricated); "logs age
//! out per level-based TTL" is real via [`types::LogRetentionPolicy`] (a real, distinct TTL per
//! [`types::LogLevel`], with a real, deliberately-chosen default — noisier levels expire sooner).
//! Both are caller-driven passes (no background scheduler runs them), matching this crate's own
//! existing "on-demand, not backgrounded" convention for `AuditLedger::verify_chain`.
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
//! - ~~**`Scheduler.subscribeLoadSignal` wiring.**~~ (2026-07-18) — now real:
//!   [`scheduler_feedback::publish_load_signal`] computes a real
//!   `hyperion_scheduler::LoadSignal` from a real [`telemetry::TelemetryCollector`]'s own
//!   recorded [`types::MetricSample`]s ([`telemetry::ewma`] over recent CPU-utilization samples,
//!   [`telemetry::derivative`] over the two most recent battery-level samples, and the single
//!   most recent thermal-headroom reading) and pushes it in via
//!   `hyperion_scheduler::Scheduler::update_load_signal` — that crate's own previously-named "no
//!   subscription API to receive one" gap, closed from that side as a plain, direct push rather
//!   than an invented callback/subscriber-list mechanism (see that crate's own doc comment for
//!   why). No Cargo cycle: `hyperion-scheduler` doesn't depend back on this crate, so this is a
//!   plain, direct new dependency, unlike that crate's own cycle-blocked `OffloadTrigger` case.
//!   Acting on the delivered signal (adaptive placement/quota decisions) remains a separate,
//!   already-named, hardware-blocked deferral in `hyperion-scheduler` itself.
//! - ~~Retention/rollup compaction of metrics and logs~~ — now real, see this crate's own "Real:"
//!   section above (`hyperion-recovery`'s own equivalent retention deferral remains separately
//!   named in that crate's own doc comment — this closes only this crate's copy of the gap).
//! - **Ring-buffer write-ahead spill on store degradation.** Still deferred.
//! - ~~Background scheduled chain verification~~ — now real:
//!   [`ledger::AuditLedger::start_periodic_verification`] spawns a real background thread that
//!   re-invokes [`ledger::AuditLedger::verify_chain`] over the whole chain every real `interval`,
//!   mirroring `hyperion-federation::FederationHub::start_lease_heartbeat`'s own `Arc<Self>`/
//!   stop-flag/join-on-drop shape exactly. A caller reads the returned
//!   [`ledger::VerificationSchedule::last_report`] instead of only ever being able to check
//!   on demand. `hyperion-observability-service`'s own real, long-running `main()` is the real
//!   consumer: it now starts one of these alongside its pre-existing on-demand startup check,
//!   for as long as the process lives.
//! - ~~A globally-unique cross-device span identity~~ — now real: [`types::SpanId`] pairs the
//!   minting collector's own `device_id` with a per-collector monotonic sequence number, and
//!   [`telemetry::TelemetryCollector::new_with_device_id`] is the real constructor a caller with
//!   a real device identity uses instead of [`telemetry::TelemetryCollector::new`] (which stays
//!   `device_id: 0`, unchanged for every existing caller). `hyperion-federation`'s
//!   `FederationHub::join_device` — the one real production call site
//!   [`telemetry::TelemetryCollector::merge_remote_trace`] feeds — now does exactly that, so two
//!   devices' collectors can never mint a colliding `span_id`, even after merging. Still a
//!   best-effort append, not a full CRDT merge (no deduplication, no conflict resolution) — this
//!   closes only the identity-collision half of that gap.

mod aggregate;
mod ledger;
mod scheduler_feedback;
mod telemetry;
mod types;

pub use aggregate::build_aggregate;
pub use ledger::{Anchor, AuditLedger, VerificationSchedule};
pub use scheduler_feedback::{
    publish_load_signal, BATTERY_LEVEL_METRIC, CPU_UTILIZATION_METRIC, THERMAL_HEADROOM_METRIC,
};
pub use telemetry::{derivative, ewma, TelemetryCollector};
pub use types::{
    AggregateReport, AuditAction, AuditLogEntry, AuditPayload, ConsentCategory, ConsentScope,
    LogEvent, LogLevel, LogRetentionPolicy, MetricRollup, MetricSample, ObservabilityError,
    PrincipalRef, RedactionClass, SpanId, SpanStatus, TraceId, TraceSpan, VerificationReport,
};
