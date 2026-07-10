# Performance Benchmarks

## Purpose

This document turns the headline performance claims of
[01 — Vision & Philosophy §10](01-vision-and-philosophy.md#10-success-criteria) — cold boot under
five seconds, near-instant wake, sub-second workspace generation, hardware ranging from
Raspberry Pi-class devices to enterprise clusters — into reproducible numbers instead of marketing
language. It specifies: a benchmark harness that decomposes each top-line target into a
per-subsystem latency or power budget; a golden trace corpus, anchored by the "prepare for
interview" trace from [02 — Core Architecture §3](02-core-architecture.md#3-how-a-request-flows-through-the-layers);
a regression-gating pipeline that feeds [34 — Observability & Telemetry](34-observability-telemetry.md)
and blocks unsafe releases in [32 — Update System](32-update-system.md); and the local-inference
throughput/latency and battery-life targets that [22 — Local AI Runtime](22-local-ai-runtime.md),
[23 — Multi-Model Orchestration](23-multi-model-orchestration.md), and
[04 — Scheduler](04-scheduler.md) are budgeted against. This document sets the *unconditional*
targets every interactive hardware tier must meet; how those targets are preserved (via
degradation) on constrained hardware is [37 — Scalability Roadmap](37-scalability-roadmap.md)'s
subject, not this one's.

## Motivation

[01 §2](01-vision-and-philosophy.md#2-the-golden-rule)'s Golden Rule applies as directly to latency
as to any other design decision: a system that reasons brilliantly about a user's goal but makes
them stare at a spinner while it does so has failed the user exactly as much as a system that
guesses wrong. [01 §10](01-vision-and-philosophy.md#10-success-criteria) is explicit that speed is
not a secondary quality attribute — "fast enough that intelligence never feels like a tax on
responsiveness" is one of five conditions for the vision being realized at all. Without this
document, those targets would be unfalsifiable slogans: every subsystem document in this
specification (03, 04, 05, and every one that follows) references "the targets in
[36 — Performance Benchmarks](36-performance-benchmarks.md)" as if they were load-bearing numbers,
which means this document must actually assign, measure, and defend those numbers, or the entire
specification's performance story is circular. A second motivation is regression prevention:
[02 §4](02-core-architecture.md#4-design-invariants) invariants are cheap to violate silently over
time (a plugin bloats cold boot by 40ms per release; nobody notices for a year) unless every
release is mechanically checked against the same budgets a human once agreed to.

## Architecture

```
┌────────────────────────────────────────────────────────────────────────────────┐
│                         GOLDEN TRACE CORPUS (version-controlled)               │
│   "prepare for interview" · "launch my startup" · boot · wake · N others       │
└───────────────────────────────────┬────────────────────────────────────────────┘
                                     ▼
┌────────────────────────────────────────────────────────────────────────────────┐
│                    BENCHMARK HARNESS  (instrumented replay)                     │
│  ┌───────────────┐   ┌────────────────────┐   ┌─────────────────────────────┐  │
│  │ Trace Replay   │──▶│ Per-Phase Timers   │──▶│ BudgetBreakdown Assembler   │  │
│  │ Driver         │   │ (03 kernel clock,  │   │ (maps timers to 03/04/05/13 │  │
│  │                │   │  not self-reported)│   │  phase boundaries)          │  │
│  └───────────────┘   └────────────────────┘   └───────────────┬─────────────┘  │
└──────────────────────────────────────────────────────────────┼──────────────────┘
                                                                 ▼
        ┌──────────────────────────────┐        ┌─────────────────────────────────┐
        │ HARDWARE MATRIX (37 tiers):   │──runs──▶│ BenchmarkResult per (spec, tier,│
        │ SBC · laptop · workstation ·  │        │ build) — percentiles, pass/fail │
        │ enterprise node               │        └───────────────┬─────────────────┘
        └──────────────────────────────┘                          ▼
                                              ┌─────────────────────────────────────┐
                                              │ 34 — OBSERVABILITY & TELEMETRY       │
                                              │ (lab results + opt-in fleet signal)  │
                                              └───────────────────┬───────────────────┘
                                                                  ▼
                                              ┌─────────────────────────────────────┐
                                              │  REGRESSION GATE (stat. significance,│
                                              │  tier-stratified baseline compare)    │
                                              └───────────────────┬───────────────────┘
                                                     pass │              │ fail
                                                          ▼              ▼
                                          ┌────────────────────┐  ┌─────────────────────┐
                                          │ 32 — UPDATE SYSTEM  │  │ Auto-bisect + block, │
                                          │ release proceeds    │  │ 33 — Rollback/Recov. │
                                          └────────────────────┘  └─────────────────────┘
```

Measurement points are always taken at kernel-provided monotonic clocks
(per [03 — Kernel Architecture](03-kernel-architecture.md)), never self-reported by the Capability
or Agent under test, so a component cannot under-report its own latency. The harness runs both as
a pre-release gate (golden trace corpus, controlled lab hardware matrix) and as a continuous,
opt-in production signal (fleet telemetry), because some regressions — storage fragmentation, real
Knowledge Graph size, thermal behavior in an actual enclosure — are not reproducible in a lab.

## Data Structures

```
BenchmarkSpec {
  id: BenchmarkId
  category: boot | wake | workspace_gen | inference | battery
  trace_ref: TraceId | null              // golden trace corpus entry, if trace-driven
  budget: BudgetTree                     // nested phase -> target_ms | target_mw
  hardware_matrix: [HardwareProfileId]   // 37 — Scalability Roadmap tiers
  sampling: { warm_runs: u32, cold_runs: u32, thermal_precondition: ThermalState }
}

BudgetTree {
  phase: string                          // e.g. "kernel_init", "resident_model_load"
  owning_doc: string                     // e.g. "03-kernel-architecture.md"
  target_ms: u32
  children: [BudgetTree]                 // sums must not exceed parent's target_ms
}

BenchmarkResult {
  spec_id: BenchmarkId
  hardware_profile: HardwareProfileId
  build_id: BuildId
  timestamp: Instant
  samples: [Duration] | [PowerReading]
  p50, p95, p99: Duration | Power
  verdict: pass | fail | quarantined
  regression_delta: f32                  // vs. rolling baseline, signed
}

RegressionGate {
  metric: BenchmarkId
  baseline_window: { builds: u32 } | { days: u32 }   // trailing window, tier-stratified
  threshold: { pct: f32 } | { sigma: f32 }
  action: block_release | warn | quarantine_and_rerun
}
```

## Algorithms

**1. Budget decomposition.** Each top-line target (cold boot, wake, workspace generation) is
decomposed along the same layer boundaries [02 §1](02-core-architecture.md#1-layered-system-view)
already defines, because those boundaries are where independent teams own independent code: a
`BudgetTree` node's `owning_doc` is always the subsystem document responsible for that phase, so a
budget overrun has an unambiguous owner. Children of a `BudgetTree` node are required to sum to at
most the parent's `target_ms`, with an explicit reserved-margin leaf (never zero) absorbing
measurement noise and cross-phase interaction the decomposition can't fully isolate.

**2. Percentile-based regression detection.** A build's `p50`/`p95` for a given
`(BenchmarkSpec, HardwareProfile)` pair is compared only against the rolling baseline for the
*same* pair — tiers are never cross-compared, since an SBC and an enterprise node are expected to
differ by design (see [37 — Scalability Roadmap](37-scalability-roadmap.md)). A regression is
flagged when the delta exceeds both a minimum practical threshold (e.g., 5% for boot, 8% for
workspace generation — smaller absolute budgets tolerate less relative drift) and a statistical
significance test against baseline variance, so a single noisy run cannot block a release and a
real, small, consistent regression cannot hide inside noise.

**3. Flake quarantine.** A run whose `ThermalState` precondition wasn't met, or whose variance
exceeds a configured coefficient-of-variation threshold, is quarantined and re-run automatically
before it counts toward a gate decision — this is what keeps thermal or scheduler jitter from
producing false gate failures without simply widening thresholds until real regressions also slip
through.

**4. Fleet-informed gating.** Passing the pre-release gate authorizes a canary rollout, not a full
release, per [32 — Update System](32-update-system.md). Canary-cohort telemetry
(opt-in, privacy-preserving per [16 — Privacy Architecture](16-privacy-architecture.md)) is checked
against the same `RegressionGate` rules; a fleet-detected regression the lab harness missed
triggers the same auto-bisect and [33 — Rollback & Recovery](33-rollback-recovery.md) path as a
pre-release failure.

## Interfaces / APIs

```
benchmark_register(spec: BenchmarkSpec) -> BenchmarkId
benchmark_run(id: BenchmarkId, profile: HardwareProfileId, build: BuildId) -> BenchmarkResult
benchmark_baseline(id: BenchmarkId, profile: HardwareProfileId) -> BaselineWindow
gate_check(build: BuildId) -> GateVerdict                     // consumed by 32 — Update System
telemetry_emit(result: BenchmarkResult) -> ()                  // to 34 — Observability & Telemetry
trace_replay(trace: TraceId, profile: HardwareProfileId) -> BenchmarkResult
explain_regression(build: BuildId, spec_id: BenchmarkId) -> RegressionReport   // 18
```

## Pseudocode

Harness sketch replaying the "prepare for interview" trace end to end, assembling a
`BudgetBreakdown`, and gating a build — this is the same shape used for the boot and wake specs,
substituting a different trace/timer set.

```python
def run_workspace_gen_benchmark(build: BuildId, profile: HardwareProfileId) -> BenchmarkResult:
    precondition_thermal_state(profile)                 # avoid contaminated "cold" numbers
    t = PhaseTimer(clock=kernel_monotonic_clock())       # 03 — kernel-provided, not self-reported

    utterance = golden_trace_corpus.load("prepare_for_interview")
    session = spawn_session(profile, build)

    t.mark("capture")
    session.emit_utterance(utterance)                    # L6, step 1 of 02 §3

    t.mark("intent_parse_start")
    intent = IntentEngine.parse(utterance, session.ctx)   # 05, step 2
    t.mark("intent_parse_end")

    t.mark("context_assemble_start")                      # runs concurrently with parse above
    ctx_bundle = ContextEngine.assemble(session)           # 06, step 2
    t.mark("context_assemble_end")

    t.mark("cap_resolution_start")
    graph = IntentEngine.decompose(intent)
    ticket = MultiAgentCoordination.submit(graph)          # 12, step 3
    resolved = [Agent.resolve_capabilities(leaf) for leaf in graph.frontier()]  # 05/23, step 4
    grounded = [KnowledgeGraph.read(obj) for obj in resolved.objects]           # 09, step 5
    scheduler.admit_all(resolved)                          # 04/03, step 6
    t.mark("cap_resolution_end")

    t.mark("ui_compile_start")
    workspace = DynamicUIRuntime.compile(graph, grounded)  # 13, step 7
    t.mark("ui_compile_end")
    t.mark("first_paint")

    breakdown = BudgetBreakdown.from_timer(t, spec=WORKSPACE_GEN_SPEC)
    result = BenchmarkResult.from_breakdown(breakdown, build, profile)
    telemetry_emit(result)                                 # 34
    return result


def gate_check(build: BuildId) -> GateVerdict:
    verdicts = []
    for spec in registered_specs():
        for profile in spec.hardware_matrix:
            result = benchmark_run(spec.id, profile, build)
            baseline = benchmark_baseline(spec.id, profile)
            delta = percent_delta(result, baseline)
            if is_flaky(result):
                result = requeue_and_rerun(spec.id, profile, build)   # quarantine, Algorithms §3
            verdicts.append(evaluate_gate(spec.gate, result, baseline, delta))
    if any(v == "block_release" for v in verdicts):
        bisection_agent.start(build, failing=[v for v in verdicts if v.blocked])
        return GateVerdict.BLOCKED
    return GateVerdict.PASS
```

## Security Considerations

The harness runs the workload under test at the same Trust Boundary and capability scope it would
have in production — it is never granted elevated privilege to "see" a faster path than a real
user session would get, which would make the benchmark meaningless. Because a Capability could in
principle detect that it is being benchmarked and return a canned fast result, the golden trace
corpus is drawn from real (opt-in, anonymized) recorded sessions replayed through the ordinary
Intent/Capability pipeline rather than invoked through a special "benchmark mode" flag visible to
Capability code — there is no code path a Capability can branch on to know it is under test.
Telemetry leaving a device is aggregated into latency/power histograms only; raw utterances,
Semantic Object contents, and per-user identifiers are never included, consistent with
[16 — Privacy Architecture](16-privacy-architecture.md)'s local-first invariant. Fine-grained
per-Capability timing is itself a potential side channel (a spike in `web.research` latency at a
precise timestamp could leak what a user is doing) — fleet-aggregate telemetry buckets timings
coarsely and adds calibrated noise before cross-device aggregation, and per-device raw traces never
leave the device by default.

## Failure Modes

- **Thermal contamination.** A "cold boot" or inference benchmark run on a device that hasn't
  returned to a baseline thermal state produces an artificially throttled result.
- **Cross-tier false regression.** Comparing an SBC's numbers against a workstation's baseline
  (rather than its own tier's) would flag expected, by-design differences as regressions.
- **Cold-cache skew on wake benchmarks.** If a wake path accidentally triggers a full model or
  index reload instead of resuming resident state, the wake benchmark silently degrades into a
  cold-boot-shaped benchmark without an explicit failure signal.
- **Network-dependent step non-reproducibility.** A trace step that falls back to a consented cloud
  Capability introduces external latency variance the harness cannot control.
- **Stale baseline.** A telemetry pipeline outage causes the regression gate to compare against an
  outdated baseline window, masking a real regression or flagging a false one.
- **Silent budget redefinition.** A change to what counts as "workspace generated" (e.g., moving
  first-paint earlier by deferring more content) can look like a performance win that is actually a
  change in what is being measured.

## Recovery Mechanisms

Flaky runs are quarantined and automatically re-run before counting toward a gate decision
(Algorithms §3), so transient noise does not require human triage. A gate failure automatically
starts a bisection run across the offending commit range and files the result to the owning
subsystem team, identified via each `BudgetTree` node's `owning_doc`. Fleet-detected post-release
regressions trigger the same [32 — Update System](32-update-system.md) canary-halt and
[33 — Rollback & Recovery](33-rollback-recovery.md) rollback path used for correctness regressions
— performance is treated as a release-blocking correctness property, not a secondary metric.
Thermal-precondition failures cause an automatic reschedule rather than a contaminated result being
recorded. Any redefinition of a measured boundary requires a `BenchmarkSpec` version bump; old and
new spec versions run in parallel for one release cycle so a discontinuity is visible rather than
silently absorbed into the trend line.

## Performance Analysis

**Cold boot budget (< 5 s target).** Firmware/bootloader handoff is outside Hyperion's control but
budgeted for; every phase below it is:

| Phase | Layer | Budget | Contents |
|---|---|---|---|
| Firmware/bootloader handoff | pre-L0 | 250 ms | UEFI/U-Boot handoff, kernel image load |
| Privileged-core init | L0 | 250 ms | Capability monitor bootstrap, address-space init, scheduling classes registered ([03](03-kernel-architecture.md)) |
| Driver bring-up | L0/L1 | 600 ms | HAL device-class enumeration; storage, display, input, network drivers spawned in parallel ([03 §Driver Model](03-kernel-architecture.md#driver-model)) |
| Platform services | L2 | 500 ms | Capability Registry, Plugin Framework bootstrap (plugin *load* deferred), Storage Engine mount, Event System online |
| Knowledge layer attach | L3 | 350 ms | Knowledge Graph index attach (lazy — not a full load), Semantic Filesystem mount |
| Cognition layer + resident model load | L4 | 1,800 ms | Context/Intent Engine ready; resident small local model loaded per [22 — Local AI Runtime](22-local-ai-runtime.md) — see breakdown below |
| Experience layer first frame | L6 | 400 ms | Compositor first frame + conversational shell input-ready (overlaps tail of L4) |
| Reserved margin | — | 350 ms | Absorbs measurement/hardware noise |
| **Total** | | **~4.5 s** | Under the 5 s target with 0.5 s margin |

Resident-model-load sub-budget (the single largest phase, per [22](22-local-ai-runtime.md)):
weight read from storage (~500 MB quantized small-tier model at ≥1.2 GB/s effective read, including
NVMe/eMMC variance across the hardware matrix) ≈ 500 ms; NPU/accelerator context and KV-cache
warmup ≈ 600 ms; first-token sanity inference (self-check, not user-visible) ≈ 300 ms; scheduler
registration of the model as an always-resident `SchedClass::InteractiveAgent` allocation
([04](04-scheduler.md)) ≈ 100 ms; remaining 300 ms is phase margin. On constrained hardware this
phase is the primary target of the degradation strategy in
[37 — Scalability Roadmap](37-scalability-roadmap.md).

**Wake-from-sleep (near-instant).** Wake is fast because almost nothing is reloaded: resident
model weights and warm KV-cache stay in self-refresh RAM ([22](22-local-ai-runtime.md)); the
foreground Workspace's compiled UI tree and last-rendered compositor surface stay resident
([13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md)); the active Intent Graph and Context Bundle
stay cached in RAM; capability tables are re-validated for liveness, not re-derived
([03](03-kernel-architecture.md)). Measured breakdown: hardware resume (RAM self-refresh exit,
panel re-init) ≈ 80 ms; capability-table liveness re-check ≈ 20 ms; compositor re-present + input
re-arm ≈ 30 ms — **perceived wake latency target: < 150 ms.** Background subsystem confirmation
(resident-model liveness ping, paused Agents resuming) completes within 400 ms but is non-blocking
for perceived interactivity.

**Workspace generation (< 1 s target), budget by bucket:**

| Bucket | Target | What it covers |
|---|---|---|
| Intent parse | 150 ms | [05 — Intent Engine](05-intent-engine.md) fast path |
| Context assembly | 100 ms | [06 — Context Engine](06-context-engine.md) bundle assembly (runs concurrently with parse; not on the critical path, excluded from the totals below) |
| Capability resolution | 300 ms | Coordination assignment ([12](12-multi-agent-coordination.md)), model routing ([23](23-multi-model-orchestration.md)), Knowledge Graph grounding reads ([09](09-knowledge-graph.md)), scheduler admission ([04](04-scheduler.md)) |
| UI compile | 250 ms | [13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md) template-cache + incremental render |
| Reserved margin | 300 ms | Absorbs measurement/hardware noise on top of the 700 ms critical-path budget |
| **Total** | **700 ms critical path + 300 ms margin = 1,000 ms** | |

Measured end-to-end for the "prepare for interview" trace ([02 §3](02-core-architecture.md#3-how-a-request-flows-through-the-layers)):

| Trace step | Layer | Bucket | Elapsed | Cumulative |
|---|---|---|---|---|
| 1. Capture utterance + identity | L6 | (pre-budget) | 20 ms | 20 ms |
| 2. Intent Engine parses graph | L4 | Intent parse | 150 ms | 170 ms |
| 2. Context Engine attaches bundle (parallel) | L4 | Context assembly | +0 ms critical-path* | 170 ms |
| 3. Coordination assigns sub-intents | L5 | Cap. resolution | 40 ms | 210 ms |
| 4. Agents resolve Capabilities via Model Router | L4 | Cap. resolution | 150 ms | 360 ms |
| 5. Capabilities read/write Semantic Objects | L3 | Cap. resolution | 90 ms | 450 ms |
| 6. Scheduler admission + kernel token check | L0-L2 | Cap. resolution | 15 ms | 465 ms |
| 7. Dynamic UI Runtime compiles & paints Workspace | L6 | UI compile | 250 ms | 715 ms |
| — | — | Margin | 285 ms | **1,000 ms** |

\* Context assembly needs only the raw utterance and session identity, not the Intent parse's
output, so it runs on a separate core concurrently and does not add serially to the critical path
— it must, however, complete before step 4, since Agents need the Context Bundle.

Steps 3–6 (Capability resolution) sum to 295 ms, inside the 300 ms bucket budget above; the
measured critical path (170 + 295 + 250 = 715 ms) leaves 285 ms of the 300 ms reserved margin
actually unused on this run — consistent with, not a violation of, the budget table.

This is time to a **usable Workspace shell with placeholders**, not time to full task completion:
the Research Agent's actual web research (step 4 onward for that sub-intent) continues streaming
results into the already-rendered Workspace afterward — conflating the two would make the < 1 s
target either meaningless (if it had to include open-ended agent work) or misleading (if users
were told "done" before results existed).

**Local inference targets, by model tier ([22](22-local-ai-runtime.md) /
[23](23-multi-model-orchestration.md)):**

| Tier | Size (quantized) | Reference hardware | Time-to-first-token | Throughput | Typical use |
|---|---|---|---|---|---|
| Tiny/Edge | 0.3–1B | SBC-class NPU (~4 TOPS) | < 150 ms | 15–25 tok/s | Wake word, tiny intent classification |
| Small resident | 1–3B | Laptop NPU/iGPU (20–40 TOPS) | < 120 ms | 30–50 tok/s | Default Intent parse/grounding |
| Large local | 7–14B | Workstation dGPU (12–24 GB VRAM) | < 300 ms | 40–80 tok/s | Deep decomposition, coding/writing Agents |
| Vision | 2–8B multimodal | Laptop NPU/dGPU | < 400 ms/image | 8–15 img/s (batched, cluster) | Screen understanding, OCR, photo grounding |
| Speech (ASR) | ~100–300M streaming | Any tier, incl. SBC | < 300 ms partial | ≤ 0.3× real-time factor | Dictation, voice shell |
| Speech (TTS) | ~100–400M | Any tier | < 200 ms first chunk | ≤ 0.5× real-time factor | Voice responses |

**Battery-life targets and methodology.** Target: on laptop-class reference hardware, a scripted
"day-in-the-life" workload (defined wake/interaction cadence, background-reasoning duty cycle,
screen-on/off ratio for a representative persona) delivers battery life within 10% of the same
hardware's traditional-OS baseline under an equivalent screen-on/idle mix, with a floor of 10+
hours mixed use. SBC-class devices, often mains-powered, are instead governed by an idle/sustained
power envelope (watts, not hours) tied to the same thermal/battery governor
([04 — Scheduler §Algorithms 3](04-scheduler.md#algorithms)) that scales
`ResourceLedger.capacity` via `battery_budget_mw`. Methodology: the workload script runs under
hardware power-rail instrumentation — never OS self-reported energy counters alone, to avoid
measurement bias — across the hardware matrix, reported as steady-state mWh/hour plus a full-script
total, and regression-gated through the same pipeline as latency benchmarks.

## Trade-offs

End-to-end trace replay is realistic but slower to run and harder to attribute a regression to one
subsystem; per-subsystem microbenchmarks (e.g., Intent Engine parse latency in isolation) are fast
and precisely attributable but can miss cross-layer interaction regressions (a scheduler change
that only manifests under real capability-resolution contention). Hyperion runs both: fast
microbenchmarks gate individual subsystem changes, full trace replay gates release candidates.
Statistically significant, tier-stratified regression thresholds are more engineering effort than a
single fixed percentage, but a fixed threshold either blocks releases on ordinary hardware jitter or
lets real small regressions compound unnoticed across many releases — Hyperion accepts the added
complexity. Lab-only benchmarking preserves perfect privacy but cannot see real-world regressions
(storage fragmentation, actual Knowledge Graph scale); fleet telemetry catches these but requires
careful, aggressive anonymization — Hyperion runs both and biases the fleet side toward heavier
aggregation, consistent with [16 — Privacy Architecture](16-privacy-architecture.md). Scripted
battery benchmarks are reproducible but less representative than real usage variance; this is
supplemented, never replaced, by opt-in real-world panel telemetry.

## Testing Strategy

The harness itself is tested, not just trusted: synthetic traces with a known, injected latency in
a fake subsystem verify that `BudgetBreakdown` attribution and gate evaluation are correct before
they are trusted against real subsystems. The full benchmark suite runs across the hardware matrix
defined in [37 — Scalability Roadmap](37-scalability-roadmap.md) — the boot/wake/workspace-generation
top-line targets are unconditional across every interactive tier, while inference throughput and
battery numbers are tier-relative per that document. The golden trace corpus, including the
interview-prep and startup-launch traces, is version-controlled and replayed on every release
candidate. Continuous, opt-in fleet dogfood telemetry cross-validates lab results against real
usage distributions. Chaos/perturbation testing injects thermal events, storage latency spikes, and
network flakiness *into benchmark runs themselves* to verify the flake-quarantine logic (Algorithms
§3) rather than only the happy path, mirroring the fault-injection testing philosophy of
[03 — Kernel Architecture](03-kernel-architecture.md) and [04 — Scheduler](04-scheduler.md). Full
methodology for non-performance testing lives in [35 — Testing Strategy](35-testing-strategy.md);
this document owns only the performance dimension of that broader strategy.

---
*Next: [37 — Scalability Roadmap](37-scalability-roadmap.md) covers how these same targets hold —
via explicit, explained degradation rather than a silent quality tax — from Raspberry Pi-class
devices to enterprise clusters.*
