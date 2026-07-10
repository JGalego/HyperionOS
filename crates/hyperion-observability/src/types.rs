use std::collections::HashMap;

use hyperion_explainability::ExplanationRecord;
use hyperion_model_router::Rationale;

pub type TraceId = u64;
pub type SpanId = u64;

/// docs/34 §1's `MetricSample`.
#[derive(Debug, Clone)]
pub struct MetricSample {
    pub name: String,
    pub value: f64,
    pub unit: String,
    pub timestamp: u64,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanStatus {
    Ok,
    Error,
    Degraded,
}

/// docs/34 §1's `TraceSpan`/`CapabilityInvocationSpan`, collapsed to one
/// struct — the extra Capability-routing fields
/// (`routing_reason`/`tokens_in`/`estimated_cost`) belong to [23 — Multi-
/// Model Orchestration](../23-multi-model-orchestration.md)'s call sites,
/// which don't exist in this crate; `attributes` is the generic
/// escape hatch a real integration would use for them.
#[derive(Debug, Clone)]
pub struct TraceSpan {
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub parent_span_id: Option<SpanId>,
    pub name: String,
    pub start: u64,
    pub end: Option<u64>,
    pub status: SpanStatus,
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedactionClass {
    None,
    Partial,
    Full,
}

/// docs/34 §1's `LogEvent`.
#[derive(Debug, Clone)]
pub struct LogEvent {
    pub level: LogLevel,
    pub message: String,
    pub redaction_class: RedactionClass,
    pub timestamp: u64,
    pub trace_id: Option<TraceId>,
}

/// docs/34 §1's `AuditLogEntry.actor`, `CapabilityId` narrowed to a plain
/// numeric ref (no dedicated newtype exists across this workspace).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrincipalRef {
    User(u64),
    Agent(u64),
    Capability(u64),
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditAction {
    Grant,
    Revoke,
    ExplainRecord,
    ConsentChange,
    AdminOverride,
    /// A real `hyperion-model-router` routing decision — see
    /// [`AuditPayload::ModelRouting`].
    ModelRouting,
}

/// docs/34 §1's `AuditLogEntry.payload` — this doc doesn't redefine
/// [18 — Explainability & Trust](../18-explainability-and-trust.md)'s
/// `ExplanationRecord`, it embeds it as-is, exactly as docs/34's own
/// extraction notes: "this doc does not redefine explainability's
/// record, it consumes it." [`AuditPayload::ModelRouting`] does the same
/// for [23 — Multi-Model Orchestration](../23-multi-model-orchestration.md)'s
/// real [`hyperion_model_router::Rationale`] — this crate's own doc
/// comment named that Rationale as real but never fed to audit/telemetry.
#[derive(Debug, Clone)]
pub enum AuditPayload {
    Explanation(ExplanationRecord),
    ModelRouting(Rationale),
    Grant {
        capability_ref: String,
    },
    Revoke {
        capability_ref: String,
        reason: String,
    },
    ConsentChange {
        grant_id: u64,
    },
    Note(String),
}

/// docs/34 §1's `AuditLogEntry` — the tamper-evident, append-only,
/// never-rolled-up ledger. `signature` (hardware-root-of-trust/Merkle-
/// anchor signing) is this crate's deferred real-crypto piece.
#[derive(Debug, Clone)]
pub struct AuditLogEntry {
    pub seq: u64,
    pub prev_hash: u64,
    pub entry_hash: u64,
    pub actor: PrincipalRef,
    pub action: AuditAction,
    pub target: Option<String>,
    pub payload: AuditPayload,
    pub timestamp: u64,
}

/// docs/34 §2's tamper-evident append: the result of `Audit.verifyChain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationReport {
    Intact,
    Corrupt { at_seq: u64 },
    Empty,
}

/// docs/34 §1's `ConsentScope.category`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentCategory {
    CrashDiagnostics,
    PerfHealth,
    FeatureUsageCounts,
    None,
}

#[derive(Debug, Clone, Copy)]
pub struct ConsentScope {
    pub category: ConsentCategory,
    pub aggregation_min_cohort: u32,
}

/// docs/34 §1's `AggregateReport` — `metric_summaries` narrowed to plain
/// `(name, value)` pairs rather than full percentile rollups.
#[derive(Debug, Clone)]
pub struct AggregateReport {
    pub cohort_size: u32,
    pub summaries: Vec<(String, f64)>,
    /// docs/34 §5's k-anonymity gate: "suppressed entirely (not
    /// partial)" — when `true`, `summaries` is always empty.
    pub suppressed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ObservabilityError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
}
