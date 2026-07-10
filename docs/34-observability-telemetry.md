# Observability & Telemetry

## Purpose

This document specifies Hyperion's observability stack: the metrics, logs, and traces that make
classical resource behavior (CPU/GPU/NPU utilization, battery, thermal — the signals
[04 — Scheduler](04-scheduler.md) consumes and produces) and AI-specific behavior (which model or
implementation the [Model Router](23-multi-model-orchestration.md) chose for a given
[Capability](02-core-architecture.md#capability) invocation, at what confidence, at what token and
cost budget) inspectable, together, as one coherent system rather than two disconnected monitoring
stacks. It also specifies the **append-only, tamper-evident audit log** that
[18 — Explainability & Trust](18-explainability-and-trust.md)'s Explanation Records and
[15 — Security Architecture](15-security-architecture.md)'s capability grants and revocations write
into — the durable record that answers "what did this system do, and on whose authority" — and the
**privacy-respecting telemetry model** that keeps everything derived from a user's private
[Semantic Objects](02-core-architecture.md#semantic-object) on-device by default, per
[16 — Privacy Architecture](16-privacy-architecture.md), unless the user explicitly opts into
anonymized, aggregate system-health reporting. Finally, it specifies the two feedback loops this
data powers: adaptive scheduling in [04 — Scheduler](04-scheduler.md) and fleet-wide trend analysis
in [37 — Scalability Roadmap](37-scalability-roadmap.md).

## Motivation

A conventional APM (application performance monitoring) stack answers "is the CPU hot, is the
request slow, did the process crash." Hyperion needs all of that — it is still, at L0-L2, a real
kernel and scheduler (see [02 — Core Architecture §1](02-core-architecture.md#1-layered-system-view))
— but it also needs to answer questions no conventional stack was built for: *which* implementation
of a Capability actually ran, *how confident* it was, *how many tokens* it spent, and *why* the
system believed that was the right choice. Without this, [18 — Explainability &
Trust](18-explainability-and-trust.md) has nothing concrete to explain from, and
[15 — Security Architecture](15-security-architecture.md)'s capability grants are unauditable
promises rather than verifiable facts. Three pressures shape the design:

1. **Trust requires a record, not a claim.** [01 — Vision & Philosophy §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable)
   requires every autonomous action to be auditable and explainable. That requires a log an
   attacker — including a compromised Agent or a dishonest plugin — cannot quietly rewrite after
   the fact, which is a stronger property than a conventional log file provides.
2. **Local-first is a privacy invariant, not a preference.** [02 — Core Architecture §4](02-core-architecture.md#4-design-invariants)
   makes local-first computation and storage a design invariant. Telemetry is exactly the kind of
   subsystem that historically leaks this invariant by accident — a crash reporter that
   "helpfully" attaches the document that was open. Hyperion's telemetry model must make that
   category of leak structurally impossible, not merely discouraged.
3. **Adaptive systems need feedback, or they cannot adapt.** [04 — Scheduler](04-scheduler.md)'s
   adaptive placement and [37 — Scalability Roadmap](37-scalability-roadmap.md)'s fleet-wide
   learning both require real observed data. A system that refuses to observe itself at all cannot
   satisfy [01 §10](01-vision-and-philosophy.md#10-success-criteria)'s "fast enough" and
   "trustworthy" criteria simultaneously — it must observe *and* protect what it observes.

## Architecture

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                             TELEMETRY SOURCES                                │
│  L0/L1  Kernel & Scheduler counters        L4  Capability invocation spans   │
│  (CPU/GPU/NPU util, battery, thermal,      (model/impl routed, confidence,   │
│   queue depth — 03, 04)                     tokens, cost, latency — 23)      │
│  L2  IPC / Storage / Event counters        L5  Explanation Records (18),     │
│  (30, 28, 31)                               capability grants/revocations    │
│                                              (15), Agent coordination (12)   │
└───────────────────────┬────────────────────────────────┬─────────────────────┘
                         │ metrics / logs / spans         │ security-relevant events
                         ▼                                ▼
        ┌───────────────────────────┐     ┌──────────────────────────────────┐
        │  Local Telemetry Pipeline  │     │      Append-Only Audit Ledger     │
        │ (sampler, redactor,        │     │   (hash-chained, tamper-evident,  │
        │  aggregator, rollup)       │     │    write-only append API)         │
        └─────────────┬─────────────┘     └────────────────┬───────────────────┘
                       │                                    │
                       ▼                                    ▼
        ┌──────────────────────────────────────────────────────────────────────┐
        │                LOCAL TELEMETRY STORE  (on-device only)                │
        │     time-series metrics · structured logs · trace index · ledger     │
        └───────┬───────────────────────┬───────────────────────┬─────────────┘
                │ read                  │ read                  │ opt-in, consented
                ▼                       ▼                       ▼
   ┌────────────────────┐  ┌───────────────────────────┐  ┌────────────────────────┐
   │ 04 — Scheduler      │  │ 18 — Explainability &      │  │  Consent Gate +         │
   │ adaptive load       │  │ Trust ("why did this       │  │  Anonymizing Aggregator │
   │ feedback signal     │  │ happen?" queries)          │  │  (16 — Privacy)         │
   └────────────────────┘  └───────────────────────────┘  └────────────┬────────────┘
                                                                        │ aggregate only,
                                                                        │ k-anonymized
                                                                        ▼
                                                          ┌──────────────────────────┐
                                                          │ 37 — Scalability Roadmap  │
                                                          │ fleet-wide perf trends     │
                                                          └──────────────────────────┘
```

The pipeline is deliberately forked at the source: general metrics/logs/traces flow through a
lossy, sampled, best-effort pipeline optimized for low overhead, while security-relevant events
(grants, revocations, Explanation Records) flow through a separate, never-sampled, durability-first
path straight into the audit ledger. Only the local store's own reads ever leave the device, and
only through the consent gate — there is no direct path from any source to the fleet service.

## Data Structures

```
MetricSample {
  source: MetricSource              // cpu.util, gpu.util, battery.level, thermal.zone0, npu.queue_depth, ...
  scope: DeviceScope | ProcessScope | CapabilityScope
  value: f64
  unit: Unit
  timestamp: Instant
  tags: Map<string, string>         // e.g. {"boundary": TrustBoundaryId}
}

TraceSpan {
  trace_id: TraceID                  // minted at Intent creation, propagated via 07 — Context Propagation
  span_id, parent_span_id: SpanID
  name: string                       // e.g. "capability.invoke:document.summarize"
  start, end: Instant
  status: ok | error | degraded
}

CapabilityInvocationSpan : TraceSpan {
  capability_id: CapabilityRef                 // 02 — Capability
  implementation_id: ImplementationRef          // which impl 23 — Model Router chose
  routing_reason: RoutingReason                 // latency-preferred | privacy-preferred | cost-preferred
  confidence: f64 | null                        // model-reported, if applicable
  tokens_in, tokens_out: u64 | null
  estimated_cost: Money | null
  trust_boundary: TrustBoundaryId               // 02/03 — where this ran
  intent_id: IntentID | null                    // 05 — which Intent this served
  agent_id: AgentRef | null                     // 11 — which Agent invoked it
}

LogEvent {
  level: trace | debug | info | warn | error
  message: string                    // never embeds raw Semantic Object content — see Security
  redaction_class: none | partial | full
  timestamp, trace_id: TraceID | null
}

AuditLogEntry {
  seq: u64                           // monotonic, gapless
  prev_hash, entry_hash: Hash        // entry_hash = H(prev_hash || canonical(payload) || seq)
  actor: PrincipalRef                // user | Agent | Capability | system
  action: grant | revoke | explain_record | consent_change | admin_override
  target: CapabilityRef | ObjectRef | null
  payload: ExplanationRecord | GrantRecord | ConsentRecord
  timestamp: Instant
  signature: Option<Signature>       // periodic anchor signature — see Algorithms
}

ConsentScope {
  category: crash_diagnostics | perf_health | feature_usage_counts | none
  aggregation_min_cohort: u32        // k-anonymity floor before any data leaves the device
  retention: Duration
  revocable: true                    // always, per 16 — Privacy Architecture
}

AggregateReport {
  cohort_size: u32                   // >= aggregation_min_cohort, or the report is suppressed
  metric_summaries: [MetricRollup]   // percentiles only — never a raw per-user series
  epoch: TimeWindow
}
```

## Algorithms

**1. Trace correlation and propagation.** A `trace_id` is minted when an Intent is created (see
[05 — Intent Engine](05-intent-engine.md)) and threaded through the Context Bundle
([07 — Context Propagation](07-context-propagation.md)) into every Agent invocation
([11 — Agent Runtime](11-agent-runtime.md)) and Capability call, including calls that continue on
another device ([21 — Distributed Execution](21-distributed-execution.md)). This makes a single
Intent's entire execution — however many Agents, Capabilities, and devices it touched —
reconstructable as one trace tree, which is what [18 — Explainability & Trust](18-explainability-and-trust.md)
queries against.

**2. Tamper-evident append.** Every audit entry's `entry_hash` commits to its predecessor, its
payload, and its sequence number, so altering any historical entry breaks every hash after it.
Periodically (every `ANCHOR_INTERVAL` entries or a fixed time window), the ledger computes a Merkle
root over the segment since the last anchor and signs it with a device-held key — backed by
hardware root of trust where available, a software key otherwise (degrading gracefully across the
hardware range in [37 — Scalability Roadmap](37-scalability-roadmap.md)). A tamperer must now forge
a signature, not just patch a hash link, and any edit downstream of the last signed anchor is
detectable by recomputing the chain — see Security Considerations for what this property does and
does not guarantee.

**3. Adaptive scheduling feedback.** A continuously-running estimator computes an EWMA
(exponentially weighted moving average) over CPU/GPU/NPU utilization, a derivative over battery
drain rate, and remaining thermal headroom from recent `MetricSample`s, and publishes a
`LoadSignal` that [04 — Scheduler](04-scheduler.md) subscribes to for adaptive placement and quota
decisions. This loop is deliberately cheap and lossy (sampled, windowed) because it sits on the
scheduler's decision path; anomaly detection can trigger a burst to full-resolution capture when a
threshold is crossed, without paying that cost continuously.

**4. Privacy filtering before aggregation.** Every field in every metric, log, or span is
classified at the source into a content class: `system-health numeric` (eligible for aggregation),
`potentially-identifying` (device/session identifiers — local-only), or `Semantic-Object-derived`
(excluded from telemetry entirely by construction). Only the first class is ever eligible to leave
the device, and only after (a) on-device aggregation into percentile rollups, (b) a k-anonymity
cohort-size gate (`cohort_size >= aggregation_min_cohort`, or the report is suppressed, not
partially sent), and (c) explicit, per-category, opt-in consent — checked at *send* time against
the current `ConsentScope`, not the scope in force when the data was collected, since consent is
always revocable per [16 — Privacy Architecture](16-privacy-architecture.md).

**5. Retention and rollup.** Raw metrics are kept at full resolution for a short window (default
24h) then compacted to percentile rollups; logs age out per level-based TTL. The audit ledger is
the one exception: it is never rolled up, summarized, or deleted by the system — only the user, via
[28 — Storage Engine](28-storage-engine.md)'s own archival controls, decides its long-term fate.

## Interfaces / APIs

```
Telemetry.recordMetric(sample: MetricSample) -> ()
Telemetry.startSpan(name, parent?) -> SpanHandle
Telemetry.endSpan(handle, status, attributes) -> ()
Telemetry.emitLog(event: LogEvent) -> ()

Audit.append(entry: AuditLogEntry) -> AuditReceipt        // synchronous, durable, never dropped
Audit.verifyChain(from_seq, to_seq) -> VerificationReport
Audit.query(filter) -> [AuditLogEntry]                     // local-only, capability-checked read

Scheduler.subscribeLoadSignal(callback) -> SubscriptionId  // consumed by 04 — Scheduler
Consent.setScope(scope: ConsentScope) -> ()
Consent.currentScopes() -> [ConsentScope]
Fleet.submitAggregate(report: AggregateReport) -> Result<(), Rejected>  // no-op unless opted in
```

`Audit.append` is the only write path into the ledger; no Capability, Agent, or plugin holds a
capability to write it directly, mirroring the "exactly one enforcement point" pattern from
[03 — Kernel Architecture](03-kernel-architecture.md#capability-security-as-the-kernel-primitive).

## Pseudocode

```python
def invoke_capability(capability_ref, args, intent_id, agent_id, ctx):
    span = Telemetry.startSpan(f"capability.invoke:{capability_ref.name}", parent=ctx.trace_id)
    impl, routing_reason = ModelRouter.select_implementation(capability_ref, ctx)   # 23
    boundary = Sandbox.resolve_trust_boundary(impl)                                 # 03

    t0 = now()
    status = "ok"
    try:
        result = impl.invoke(args, boundary)
        return result
    except CapabilityFault:
        status = "error"
        raise
    finally:
        Telemetry.endSpan(span, status, CapabilityInvocationSpan(
            capability_id=capability_ref, implementation_id=impl.id,
            routing_reason=routing_reason,
            confidence=getattr(locals().get("result"), "confidence", None),
            tokens_in=getattr(locals().get("result"), "tokens_in", None),
            tokens_out=getattr(locals().get("result"), "tokens_out", None),
            estimated_cost=impl.cost_model.estimate(args),
            trust_boundary=boundary.id, intent_id=intent_id, agent_id=agent_id,
        ))
        Telemetry.recordMetric(MetricSample(
            source="capability.latency_ms", scope=capability_ref,
            value=(now() - t0).ms(), unit=MS, timestamp=now(),
            tags={"implementation": impl.id, "boundary": boundary.id},
        ))


def append_audit_entry(actor, action, target, payload):
    # Never best-effort: a grant/revoke or Explanation Record not durably logged
    # before it takes effect is treated as not having happened — mirrors 02 §4's
    # "no silent authority" invariant.
    with audit_ledger.lock():
        prev = audit_ledger.tail()
        entry = AuditLogEntry(
            seq=prev.seq + 1, prev_hash=prev.entry_hash,
            actor=actor, action=action, target=target, payload=payload,
            timestamp=now(), signature=None,
        )
        entry.entry_hash = hash(entry.prev_hash, canonical(entry.payload), entry.seq)
        audit_ledger.write_fsync(entry)
        if entry.seq % ANCHOR_INTERVAL == 0:
            root = merkle_root(audit_ledger.segment(prev.anchor_seq, entry.seq))
            entry.signature = device_key.sign(root)
    return AuditReceipt(entry.seq, entry.entry_hash)


def adaptive_scheduler_feedback(window=WINDOW_30S):
    # Continuous loop feeding 04 — Scheduler's placement/quota decisions.
    load = LoadEstimate(
        cpu=ewma(Telemetry.query_metric("cpu.util", window)),
        gpu=ewma(Telemetry.query_metric("gpu.util", window)),
        battery_drain_rate=derivative(Telemetry.query_metric("battery.level", window)),
        thermal_headroom=thermal_budget() - ewma(Telemetry.query_metric("thermal.zone*", window)),
        inference_queue_depth=Telemetry.query_metric("npu.queue_depth", window).latest(),
    )
    Scheduler.publish_load_signal(load)
```

## Security Considerations

The ledger's tamper-evidence rests on hash chaining plus periodic signed anchors; its guarantee is
**detectable tampering of the past**, not an attacker-proof present — a fully compromised device
can still forge entries going forward from the moment of compromise, which is why an optional,
user-controlled off-device anchor export bounds the blast radius further. Reading the ledger is
itself a privileged operation: no Capability holds it by default, and a plugin cannot read another
Capability's invocation trace without an explicit grant, consistent with
[15 — Security Architecture](15-security-architecture.md). Telemetry is a documented exfiltration
side channel in its own right — even metadata like token counts can leak content length or
structure — so any field derived from a Semantic Object is excluded from spans and logs by
construction (`redaction_class`), and fields eligible for fleet aggregation are bucketed/quantized
before they are allowed to cross a Trust Boundary, never sent as raw values. Full threat coverage,
including telemetry-specific attack patterns, is enumerated in
[17 — Threat Model](17-threat-model.md).

## Failure Modes

- **Pipeline overload or crash.** The lossy metrics/logs path fails open — it drops or samples
  rather than blocking any Intent's execution (Design Invariant 5, [02 §4](02-core-architecture.md#4-design-invariants))
  — but the audit path fails *closed on the action*: if durable append is unavailable, a grant or
  revocation is queued or blocked rather than proceeding unlogged.
- **Clock skew across devices** ([21 — Distributed Execution](21-distributed-execution.md))
  complicates cross-device trace merging; ordering within the ledger relies on `seq` plus device
  identity, not wall-clock trust, with wall clock retained for display only.
- **Storage pressure.** If the audit ledger's reserved quota fills, this is a first-class incident
  distinct from general disk pressure, since the ledger must never be evicted like a cache (see
  [28 — Storage Engine](28-storage-engine.md)).
- **Undetected chain corruption** if verification only runs on demand — mitigated by scheduled
  background verification, not only on-demand checks.
- **Sampling bias** can hide a rare, severe signal (a brief thermal spike between samples) —
  addressed by anomaly-triggered burst capture.
- **Fleet network partition** simply queues the aggregate locally; it never blocks local
  functionality.

## Recovery Mechanisms

Metrics and logs spill to a local write-ahead ring buffer when the store is degraded and replay on
recovery, following the same supervisor-tree pattern used for driver recovery in
[03 — Kernel Architecture](03-kernel-architecture.md#recovery-mechanisms). A detected ledger break
is never silently repaired — the affected segment is marked suspect, the user and
[15 — Security Architecture](15-security-architecture.md) are notified, and the last valid signed
anchor bounds how much history is in question. The audit ledger's storage quota is reserved and
independently alerted on, separate from general low-disk warnings. Under backpressure, the pipeline
sheds the lowest-priority streams first (raw high-frequency samples), preserving rollups and every
audit-classified event unconditionally. Fleet submissions retry with exponential backoff and
re-check consent and cohort size at send time, since both can change between collection and
submission.

## Performance Analysis

Per-invocation span overhead targets low tens of microseconds — negligible against model inference
latency, consistent with the budgets in [36 — Performance Benchmarks](36-performance-benchmarks.md).
Total observability CPU overhead targets under 1-2% steady-state, with brief spikes during
anomaly-triggered detail capture. Audit appends are fsync-bound by design; grants that occur in
quick succession may batch their fsync, but a single grant that gates whether an action proceeds
pays that latency directly rather than being buffered past the decision point (see Trade-offs).
Storage growth is bounded by the retention/rollup policy for metrics and logs; the ledger grows
linearly and unboundedly by design, with archival/export left to the user via
[28 — Storage Engine](28-storage-engine.md). Local queries (dashboards, explainability lookups) are
indexed by `trace_id`/`intent_id` for near-constant-time retrieval.

## Trade-offs

- **Synchronous audit durability vs. grant latency.** Durability wins, because it is a security
  invariant rather than a UX preference; the cost is kept small by targeting a fast local NVMe
  fsync path rather than by relaxing the guarantee.
- **Rich per-token instrumentation vs. storage/privacy cost.** Default telemetry stores counts and
  aggregates, never raw content; full per-token capture exists only as an explicit, time-boxed,
  user-initiated debug mode.
- **Local-only privacy vs. fleet-wide learning speed.** Strict opt-in, default-off consent with a
  k-anonymity floor is chosen over faster systemic learning, in direct service of the local-first
  invariant in [02 §4](02-core-architecture.md#4-design-invariants).
- **Sampling vs. full capture for the scheduler feedback loop.** EWMA-over-sampled-window is chosen
  for the hot path; full-resolution capture is reserved for anomaly-triggered bursts and post-hoc
  analysis, not the steady-state loop.
- **Hash-chain-only vs. hardware-rooted anchoring.** The hash chain is always present; hardware
  anchoring (TPM/secure enclave) is used opportunistically, so the guarantee degrades gracefully
  rather than being unavailable on lower-end hardware in [37 — Scalability Roadmap](37-scalability-roadmap.md).

## Testing Strategy

Fault injection kills the telemetry pipeline mid-write and asserts no audit gap and no grant
silently proceeding, and that ring-buffer replay recovers exactly the lost span. Tamper simulation
directly mutates a historical ledger entry in test fixtures and asserts `Audit.verifyChain` detects
the break at the correct sequence rather than accepting it. A consent-boundary canary test injects
a synthetic Semantic-Object-derived field into the pipeline and asserts it never appears in any
`AggregateReport` sent to the fleet service — run automatically in CI as an exfiltration canary. A
cohort-gate test verifies that an `AggregateReport` is suppressed entirely, not merely redacted,
whenever `cohort_size` falls below `aggregation_min_cohort`. Load testing the adaptive-scheduler
feedback loop with synthetic load signals confirms [04 — Scheduler](04-scheduler.md)'s placement
responds within its target latency, cross-checked against that document's own suite. A long-run
soak test validates the storage growth and rollup model over simulated months of usage. Finally, a
completeness invariant checker cross-references every capability grant, revocation, and Explanation
Record produced during a test run against the ledger, asserting a strict one-to-one correspondence
rather than merely "some entries exist" — the property [18 — Explainability & Trust](18-explainability-and-trust.md)
ultimately depends on.

---
*Next: [35 — Testing Strategy](35-testing-strategy.md).*
