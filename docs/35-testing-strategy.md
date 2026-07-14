# Testing Strategy

## Purpose

**Living instance of this document's own regression-capture loop:** [USAGE_SCENARIOS.md](../USAGE_SCENARIOS.md)
(repo root) is the running, human-readable log of real scenarios actually driven against the
compiled system — the concrete, growing counterpart to this document's `GoldenIntentCase`
corpus concept, until a real automated `GoldenCorpus` harness exists to replace it.

This document specifies how Hyperion is tested — a qualitatively different problem from testing a
conventional kernel, because Hyperion is deterministic at the bottom and genuinely probabilistic at
the top (see the layer model in [02 — Core Architecture §1](02-core-architecture.md#1-layered-system-view)).
It covers five distinct testing regimes, one per layer band: conventional deterministic testing for
L0-L2 (kernel, scheduler, IPC, storage); golden-path Intent regression suites with statistical
tolerance for L4-L6; a model evaluation harness that gates entries into
[23 — Multi-Model Orchestration](23-multi-model-orchestration.md)'s routing table; adversarial and
chaos testing for [12 — Multi-Agent Coordination](12-multi-agent-coordination.md) and
[21 — Distributed Execution](21-distributed-execution.md); automated accessibility conformance
linting against every generated Workspace template ([13](13-dynamic-ui-runtime.md) /
[14](14-accessibility.md)); and security regression testing against
[17 — Threat Model](17-threat-model.md)'s attack catalog. It also covers how the testing
infrastructure itself is validated, since a test suite nobody has verified can actually fail is not
evidence of anything.

## Motivation

A traditional OS test suite can assert exact equality: given input X, kernel state must be exactly
Y. Hyperion cannot make that promise above L3 — the same Intent, run twice, may route to a
different model, produce a differently-worded but equally correct summary, or take a different but
equally valid path through an Agent's reasoning. Two conclusions follow, and both are load-bearing
for the rest of this document:

1. **L0-L2 must remain fully deterministic and conventionally testable, without exception.** A
   scheduler race, a corrupted IPC message, or a storage engine bug is never acceptable to describe
   as "probabilistic" — correctness at these layers is non-negotiable, per
   [03 — Kernel Architecture](03-kernel-architecture.md)'s Testing Strategy and
   [02 — Core Architecture §4](02-core-architecture.md#4-design-invariants). Any temptation to
   loosen assertions here because "the rest of the system is fuzzy" is rejected outright.
2. **L4-L6 requires a genuinely different test philosophy**, not a weaker version of the same one:
   statistical tolerance over repeated runs, golden corpora that grow from real regressions,
   evaluation gates for the models themselves, and adversarial simulation for multi-Agent
   coordination — because these layers are probabilistic and adaptive by design
   ([01 — Vision & Philosophy §3](01-vision-and-philosophy.md#3-why-now)), and pretending otherwise
   produces either constant false failures or a false sense of exactness neither serves.

## Architecture

```
┌────────────────────────────────────────────────────────────────────────────┐
│  L6  Accessibility Conformance Linter        (every generated Workspace      │
│      + golden-path UI shape assertions         template — 13 / 14)          │
├────────────────────────────────────────────────────────────────────────────┤
│  L5  Adversarial / Chaos Harness              (Agent failure injection,     │
│      Threat-Model Regression Suite             network partitions — 12/21/17)│
├────────────────────────────────────────────────────────────────────────────┤
│  L4  Golden-Path Intent Regression Corpus     (Intent → Workspace /          │
│      Model Evaluation Harness (gates 23)        Capability trace, statistical│
│                                                  tolerance — 05 / 22 / 23)    │
├────────────────────────────────────────────────────────────────────────────┤
│  L3  Knowledge Graph / Semantic FS            (referential integrity,        │
│      consistency & property tests               schema migration — 09 / 10) │
├────────────────────────────────────────────────────────────────────────────┤
│  L0-L2  Deterministic unit / integration /    (kernel, scheduler, IPC,       │
│         property / fuzz / formal proof          storage — 03/04/30/28)      │
│         — exact-match, must pass, never waived                              │
└──────────────────────────┬───────────────────────────┬────────────────────┘
                            │ every commit               │ nightly / pre-release
                            ▼                             ▼
                  Fast deterministic gate      Statistical + adversarial + a11y gate
                            │                             │
                            └──────────────┬──────────────┘
                                           ▼
                          Release candidate → 32 — Update System
                       (staged rollout / canary; 34 — Observability feeds
                        real-world regression signal; 33 — Rollback if needed)
```

The pyramid is layer-aligned rather than shape-aligned: it is not "more unit tests than integration
tests" in the classical sense, but "more exact-match tests at deterministic layers, more
statistical/adversarial tests at probabilistic layers," with the gate between L3 and L4 being where
the testing philosophy itself changes.

## Data Structures

```
DeterministicTestCase {
  id, layer: L0 | L1 | L2
  kind: unit | integration | property | fuzz_seed | formal_obligation
  target_component: string              // e.g. "capability_monitor", "ipc.endpoint_send"
  assertion: ExactMatchAssertion
  must_pass: true                       // never waived, never quarantined
}

GoldenIntentCase {
  id, corpus_tags: [string]             // e.g. "interview-prep" (05's worked example), "startup-formation"
  utterance: string
  seed_context_bundle: ContextBundleFixture        // 06 / 07
  expected_workspace_shape: WorkspaceShapeSpec      // structural, not pixel-exact — 13
  expected_capability_trace: [CapabilityTracePattern]
  tolerance: {
    trace_edit_distance_max: u32,
    output_embedding_similarity_min: f64,
    confidence_floor: f64,
    latency_budget_ms: u32,
    required_pass_rate: f64             // e.g. 0.9 across k repeated runs
  }
  captured_from_incident: IncidentRef | null   // regression-capture provenance, see Recovery
}

ModelEvalRecord {
  model_id, task_suite: string
  accuracy, calibration_error: f64
  latency_p50_ms, latency_p99_ms: u32
  cost_per_1k_tokens: Money
  safety_flags: [SafetyCategory]
}

ChaosScenario {
  id
  fault_type: agent_crash | network_partition | message_loss | clock_skew | resource_starvation
  target: AgentRef | TrustBoundaryId | LinkRef
  injected_at: TriggerPolicy            // random exploration or targeted known-fragile replay
  expected_invariants: [InvariantSpec]   // e.g. "no two Agents hold the goal-state write lock"
}

AccessibilityRule {
  rule_id: string                        // maps to a 14 — Accessibility requirement
  severity: blocking | warning
  applies_to: WorkspaceTemplateSelector
}

ThreatRegressionCase {
  threat_id: string                      // from 17 — Threat Model's catalog
  attack_vector: ReplayableAttack
  expected_mitigation: MitigationAssertion
  provenance: never_tested | previously_mitigated
}
```

## Algorithms

**1. Statistical golden-path comparison.** Each `GoldenIntentCase` runs `k` times against a
candidate build. A single run passes only if every tolerance dimension holds simultaneously (trace
edit distance, output embedding similarity, confidence floor, latency budget). The case as a whole
passes if the observed pass rate meets `required_pass_rate`; a pass rate in a narrow band just
below that threshold is quarantined for human triage rather than auto-failed or auto-passed,
because at this layer a single bad run can be sampling noise, not a regression.

**2. Model gating for [23 — Multi-Model Orchestration](23-multi-model-orchestration.md)'s routing
table.** A candidate model is evaluated offline against fixed accuracy/latency/cost/safety
thresholds relative to the current champion. Candidates that clear the offline bar enter **shadow
mode**: run in parallel against live or replayed traffic with zero influence on actual routing
decisions, purely to measure real-world divergence from the champion — gated by the identical
`privacy_gate` check live routing uses (23's `shadow_evaluate`), so a candidate that wouldn't be
permitted to see a given Context Bundle live never receives it in shadow either. Only after a clean shadow
soak period is a candidate admitted at a small canary routing percentage, ramped per
[32 — Update System](32-update-system.md)'s staged rollout, with
[34 — Observability & Telemetry](34-observability-telemetry.md) supplying the live regression signal
that can halt the ramp automatically.

**3. Chaos scheduling for [12 — Multi-Agent Coordination](12-multi-agent-coordination.md) and
[21 — Distributed Execution](21-distributed-execution.md).** Fault injection combines randomized
exploration of fault type/target/timing with targeted replay of a growing **known-fragile
registry** — interaction points where a real incident was previously found (mirroring the golden
corpus's own regression-capture loop). After each injection, the harness checks invariants over the
shared goal state itself (Intent Graph consistency, no doubly-assigned sub-intent, no orphaned
lock) rather than merely confirming that crashed Agents restarted.

**4. Accessibility conformance linting.** Runs both in CI and inline in the
[Dynamic UI Runtime](13-dynamic-ui-runtime.md)'s own template generation path, evaluating the
generated structure — contrast ratios, focus-order reachability from every interactive element,
semantic labeling presence, motion/timing thresholds — against [14 — Accessibility](14-accessibility.md)'s
ruleset. Blocking-severity violations fail generation or the build; warning-severity violations are
logged and trended rather than gating.

**5. Threat-model regression replay.** Every catalogued item in
[17 — Threat Model](17-threat-model.md) has a replayable attack fixture, run on every release
candidate. A previously-mitigated attack that stops being mitigated is a release-blocking
regression, distinguished by provenance metadata from an attack that has simply never been
catalogued yet (a gap, not a regression, and tracked differently).

## Interfaces / APIs

```
DeterministicHarness.run(layer: L0|L1|L2) -> SuiteResult

GoldenCorpus.run(case: GoldenIntentCase, build: BuildRef, k: u32) -> StatisticalResult
GoldenCorpus.capture(incident: ProductionRegression) -> GoldenIntentCase

ModelEvalHarness.evaluate(candidate: ModelRef, suite: TaskSuiteRef) -> ModelEvalRecord
ModelEvalHarness.gate(candidate: ModelEvalRecord, champion: ModelEvalRecord) -> GateDecision
ModelEvalHarness.shadow(candidate: ModelRef, traffic: TrafficSampleRef) -> ShadowReport

ChaosHarness.inject(scenario: ChaosScenario) -> InvariantReport
AccessibilityLinter.lint(template: WorkspaceTemplate) -> [Violation]
ThreatRegression.replay(threat_id: string, build: BuildRef) -> PassFail

ReleaseGate.evaluate(build: BuildRef) -> ReleaseDecision   // aggregates all of the above, feeds 32
```

## Pseudocode

```python
def run_golden_case(case, build, k=None):
    k = k or case.tolerance.get("k", 7)
    outcomes = []
    for _ in range(k):
        ctx = materialize(case.seed_context_bundle)                     # 06 / 07
        graph = IntentEngine.decompose(IntentEngine.parse(case.utterance, ctx))  # 05
        ticket = MultiAgentCoordination.submit(graph)                    # 12
        workspace, trace = execute_to_completion(ticket, build)          # 13, traced per 34

        outcomes.append(all([
            shape_matches(workspace, case.expected_workspace_shape),
            edit_distance(trace, case.expected_capability_trace)
                <= case.tolerance.trace_edit_distance_max,
            embedding_similarity(workspace.primary_output, case.expected_workspace_shape.reference)
                >= case.tolerance.output_embedding_similarity_min,
            min(s.confidence for s in trace if s.confidence is not None)
                >= case.tolerance.confidence_floor,
            trace.total_latency_ms <= case.tolerance.latency_budget_ms,
        ]))

    pass_rate = sum(outcomes) / k
    if pass_rate >= case.tolerance.required_pass_rate:
        return StatisticalResult.PASS
    elif pass_rate >= case.tolerance.required_pass_rate - FLAKY_BAND:
        return StatisticalResult.QUARANTINE          # triage, do not block release yet
    return StatisticalResult.FAIL


def gate_candidate_model(candidate, champion, suite):
    record = ModelEvalHarness.evaluate(candidate, suite)                 # 22 / 23
    baseline = ModelEvalHarness.evaluate(champion, suite)

    if (record.accuracy < baseline.accuracy - MAX_ACCURACY_REGRESSION
            or record.latency_p99_ms > baseline.latency_p99_ms * MAX_LATENCY_MULTIPLIER
            or record.cost_per_1k_tokens > CAPABILITY_COST_CEILING[suite.capability_class]
            or record.safety_flags):
        return GateDecision.REJECTED

    shadow = ModelEvalHarness.shadow(candidate, traffic=recent_live_traffic_sample())
    if shadow.divergence_rate > MAX_SHADOW_DIVERGENCE:
        return GateDecision.HOLD_FOR_REVIEW

    return GateDecision.admit_at_canary_percent(INITIAL_CANARY_PCT)      # ramps via 32


def run_chaos_scenario(scenario, live_system):
    pre_state = snapshot_shared_goal_state(live_system)                  # Intent Graph + locks, 12
    fault_injector.apply(scenario)
    wait_for_quiescence_or_timeout(live_system, CHAOS_TIMEOUT)

    post_state = snapshot_shared_goal_state(live_system)
    violations = [inv for inv in scenario.expected_invariants
                  if not inv.holds(pre_state, post_state)]
    fault_injector.clear(scenario)                                       # always clean up injected faults
    return InvariantReport(scenario.id, violations, recovered=live_system.is_healthy())
```

## Security Considerations

Test and evaluation corpora are themselves an attack surface: an attacker who can poison the golden
corpus or an eval dataset could get a regressed model or a malicious Capability waved through a
gate — a supply-chain attack on CI, not on production. Corpus entries are provenance-signed and
writable only under the same capability checks as any other Semantic Object
([15 — Security Architecture](15-security-architecture.md)), with a held-out, rotating portion never
exposed to model or Capability developers, specifically to resist "teaching to the test." Chaos and
threat-regression harnesses necessarily execute real attack payloads and real fault injection
against a system holding real capability tokens; both run inside an isolated, ephemeral Trust
Boundary (depth 2 or 3 in [03 — Kernel Architecture](03-kernel-architecture.md#sandboxing-as-one-spectrum))
that cannot reach production Semantic Objects, and that is itself denied every capability it is not
explicitly exercising. Golden-path cases captured from real production incidents (the
regression-capture loop, below) are subject to the same consent gate as
[34 — Observability & Telemetry](34-observability-telemetry.md)'s aggregate reporting before they
enter a shared corpus — a captured incident is anonymized and consented, not silently harvested.
Every release-gate decision is itself written to 34's append-only audit ledger, so "why was this
model or Capability version shipped" is answerable exactly like any other autonomous decision, per
[18 — Explainability & Trust](18-explainability-and-trust.md).

## Failure Modes

- **Flaky golden-path cases**, where sampling variance is misclassified as a regression, or a real
  regression hides inside noise. The quarantine band addresses the first; the second is a genuine
  risk if tolerance is loosened carelessly, which is why tolerance changes require review (see
  Recovery).
- **Eval-harness gaming**: a candidate model tuned against a stable, known golden corpus scores well
  in gating but regresses on live traffic it was never evaluated against.
- **Unrepresentative chaos topology**: a staging environment that doesn't reproduce production-scale
  network conditions ([21 — Distributed Execution](21-distributed-execution.md)) can miss failure
  modes that only appear at scale.
- **Accessibility linter false negatives** on novel generative UI shapes the ruleset has not yet
  encountered, since [13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md) constantly produces new
  structures.
- **Stale threat regression suite**: [17 — Threat Model](17-threat-model.md)'s catalog is a living
  document, and a suite that only replays yesterday's attacks provides false assurance against
  tomorrow's.
- **CI cost/time explosion**: running the full statistical corpus, full model evaluation, and full
  chaos matrix on every commit is not tractable — see Performance Analysis for the resulting
  scheduling trade-offs.

## Recovery Mechanisms

Quarantined flaky cases are automatically retried with an increased `k` and routed to human triage
rather than either silently passing or blocking every release; a case that recurs as flaky triggers
review of its tolerance definition itself, not just a rerun. The **regression-capture loop**: any
production incident surfaced by [34 — Observability & Telemetry](34-observability-telemetry.md)'s
post-deployment monitoring — a real Intent that produced a bad Workspace, caught by user correction
or anomaly detection — is captured back into the golden corpus as a new `GoldenIntentCase`, so the
corpus grows from real failures, not only hand-authored ones. Canary ramps from the model-gating
algorithm are halted and rolled back ([33 — Rollback & Recovery](33-rollback-recovery.md)) the
moment 34's live feed shows regression on accuracy, latency, or user-correction rate, without
waiting for the next test cycle. Corpus and eval-set integrity is periodically re-verified against
signed provenance, with a known-good snapshot restorable from version control if tampering is
detected.

## Performance Analysis

The deterministic L0-L2 suite must complete within commit-gating time (minutes) — correctness there
is non-negotiable and cannot be deferred to a nightly run — using property-based and fuzz testing
for broad coverage cheaply, while the slower, formal-verification obligations
([03 — Kernel Architecture](03-kernel-architecture.md#testing-strategy)) run on a cadence tied to
changes in that specific component rather than every commit. The full golden-path corpus at full
`k`-repeat statistical depth is expensive (model inference cost × corpus size × k) and runs nightly
and pre-release; every pull request runs a fast, historically-highest-value subset to keep PR
feedback tight. The model evaluation harness is the most expensive tier and runs on a
new-candidate cadence gated into [32 — Update System](32-update-system.md)'s release cycle, not
CI's. The chaos suite runs on merge to a release branch and periodically in a dedicated
environment, since standing up multi-Agent, multi-node fault scenarios is costly; the known-fragile
registry lets a cheaper, high-value subset run more frequently. Accessibility linting is cheap
enough to run inline in the Dynamic UI Runtime's own generation path in production, giving it the
tightest feedback loop of any test in this document.

## Trade-offs

- **Exact-match (L0-L2) vs. statistical tolerance (L4-L6).** Statistical tolerance loses precision
  but is the only workable model for genuinely probabilistic output; the boundary is drawn at the
  L3/L4 transition, with L3's knowledge-graph consistency tests sitting between the two as
  property/invariant checks rather than either extreme.
- **Corpus breadth vs. CI velocity.** An ever-growing regression-captured corpus increases
  confidence but slows the full nightly run; the representative-subset-per-PR strategy accepts
  slightly delayed full-corpus feedback in exchange for fast iteration.
- **Synthetic vs. real-world test data.** Synthetic golden cases are cheap, safe, and fully
  controllable but can miss real usage patterns; real captured incidents are higher-fidelity but
  carry the same privacy and consent obligations as [16 — Privacy Architecture](16-privacy-architecture.md),
  which bounds how fast the corpus can grow from real data.
- **Chaos coverage vs. combinatorial explosion.** Every fault/timing/target combination cannot be
  tested; prioritizing known-fragile-path replay over exhaustive random exploration is a deliberate,
  accepted incompleteness.
- **Model-gating strictness vs. iteration speed.** Longer shadow-soak periods reduce the risk of a
  bad model reaching users but slow adoption of a genuinely better one; the canary-ramp mechanism is
  the chosen middle ground — fast entry, bounded blast radius if wrong.

## Testing Strategy

Since this entire document's subject *is* testing, the meta-question — how do we know the test
infrastructure itself works — is answered directly rather than assumed. **Mutation testing on the
harness**: a known bug is deliberately planted in a shadow build (a broken capability-grant check, a
misrouted Capability, a contrast-ratio violation) and the relevant suite must catch it; a suite that
fails to catch a planted, known regression is itself a release-blocking finding about the suite, not
about the shadow build. **Chaos-of-chaos**: the chaos harness's own control plane is fault-injected
— killed mid-injection, partitioned from the system under test — and must fail safe, clearing any
fault it had injected rather than leaving the system under test permanently faulted. **Periodic
corpus and eval-set audits**: a human red-team, on a fixed cadence, samples the golden corpus and
eval sets for staleness, overfitting, or drift against real production Intent patterns surfaced by
[34 — Observability & Telemetry](34-observability-telemetry.md), since automated corpus growth can
silently drift away from what users actually do. **Controlled canary regression drills**:
periodically ship a deliberately flawed build to an internal-only canary ring specifically to
confirm the release-gate, staged-rollout, and rollback pipeline
([32](32-update-system.md)/[33](33-rollback-recovery.md)/[34](34-observability-telemetry.md)) halts
and reverts it end-to-end — proof the pipeline works as a whole, not just that its individual
suites pass in isolation. Finally, a **completeness invariant**: every release-gate decision must
correspond to exactly one signed entry in 34's audit ledger, checked by a completeness verifier over
the ledger and release history, so no release ever ships without a recorded, passing gate
evaluation.

---
*Next: [36 — Performance Benchmarks](36-performance-benchmarks.md).*
