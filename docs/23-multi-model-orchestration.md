# Multi-Model Orchestration

## Purpose

This document specifies the **Model Router**, the L4 Cognition Layer component introduced in
[02 — Core Architecture §1](02-core-architecture.md#1-layered-system-view) and named explicitly in
the worked trace at [02 §3, step 4](02-core-architecture.md#3-how-a-request-flows-through-the-layers):
"Each Agent resolves its sub-intent by invoking Capabilities … chosen and routed by the Model
Router." Given a [Capability](02-core-architecture.md#capability) invocation with a declared
semantic contract, the Model Router decides *which implementation* satisfies it — a local small
model, a local large model, a cloud API, a native binary, or a Capability composed from others —
and does so under hard privacy constraints, real-time resource constraints, and an explicit
fallback discipline. This document does not cover *how* a chosen model executes on-device
(quantization, VRAM residency, hardware adaptation); that is
[22 — Local AI Runtime](22-local-ai-runtime.md)'s domain. This document owns the *decision*, not
the execution.

## Motivation

[02 — Capability](02-core-architecture.md#capability) is explicit that "the OS — not the user, and
not the developer — chooses which implementation of a Capability satisfies a given Intent at a
given moment." Left unspecified, that sentence is a wish; this document is the mechanism. Four
pressures make the routing decision hard enough to need a dedicated subsystem rather than a static
lookup table:

1. **Design Invariant 3** ([02 §4](02-core-architecture.md#4-design-invariants)): local-first by
   default, cloud is an explicit consented upgrade, *never a silent fallback*. A router that
   quietly reaches for a cloud API because the local model is momentarily busy violates this
   invariant as surely as one that ignores privacy settings outright.
2. **Design Invariant 5**: degrade, never fail closed. If the ideal implementation is unavailable
   — VRAM exhausted, thermal throttled, offline — the system must substitute a lesser one and say
   so, not block the user's goal.
3. **Heterogeneous candidates with incomparable units.** A local 3B-parameter model, a local
   70B-parameter model, a metered cloud API, and a deterministic native binary differ in latency,
   cost, quality, and privacy exposure by orders of magnitude along each axis simultaneously. There
   is no single natural ordering; the router must make an explicit, inspectable trade.
4. **Stakes vary per invocation.** Routing "summarize this meme" and "review this contract clause
   before I sign" through the same undifferentiated logic is a category error. High-stakes
   Capability calls need verification patterns, not just a single best guess, and the reasoning
   behind the choice must be reconstructable for
   [18 — Explainability & Trust](18-explainability-and-trust.md)'s "alternatives considered."

## Architecture

The Model Router sits in the Cognition Layer (L4) alongside the
[Intent Engine](05-intent-engine.md), [Context Engine](06-context-engine.md),
[Memory Engine](08-memory-engine.md), and [Agent Runtime](11-agent-runtime.md). It is invoked once
per Capability call — never once per Intent — because a single Intent typically decomposes into
many Capability invocations (per the interview-prep trace in
[02 §3](02-core-architecture.md#3-how-a-request-flows-through-the-layers)), each of which may
warrant a different implementation choice as conditions change mid-task.

```
                         ┌─────────────────────────────────┐
                         │  Agent Runtime (11) / Intent      │
                         │  Engine (05) emit a               │
                         │  CapabilityInvocation             │
                         └────────────────┬───────────────────┘
                                          │
                                          ▼
┌────────────────────────────────────────────────────────────────────────────┐
│                       MODEL ROUTER  (L4, this document)                     │
│                                                                              │
│  ┌────────────────┐    ┌───────────────────┐    ┌───────────────────────┐  │
│  │ Candidate        │──▶│ Feasibility &       │──▶│ Scoring Engine          │  │
│  │ Gathering         │   │ Privacy Gate         │   │ latency · privacy ·    │  │
│  │ (queries Plugin   │   │ (hard filter, never  │   │ cost · quality ·       │  │
│  │ Framework's        │   │ scored around —      │   │ availability           │  │
│  │ Capability          │   │ ties to 16, 04, 22)  │   └───────────┬────────────┘  │
│  │ Registry, 24)       │   └───────────────────┘                  │               │
│  └────────────────┘                                              ▼               │
│                                                    ┌───────────────────────────┐  │
│                                                    │ Decision: top pick +        │  │
│                                                    │ fallback chain + optional   │  │
│                                                    │ ensemble/verification group │  │
│                                                    └──────────────┬───────────────┘  │
└───────────────────────────────────────────────────────────────────┼───────────────────┘
                                                                     │
                  ┌──────────────────────────────────────────────────┼───────────────────────┐
                  ▼                                                    ▼                        ▼
        ┌───────────────────┐                            ┌───────────────────────┐   ┌───────────────────────┐
        │ Scheduler (04)       │                            │ Explainability &        │   │ Benchmark table,        │
        │ dispatches the        │                            │ Trust (18) receives     │   │ fed by field telemetry  │
        │ chosen implementation │                            │ the rationale as         │   │ and staged rollout via  │
        │ inside its Trust       │                            │ "alternatives            │   │ Update System (32)      │
        │ Boundary (03)          │                            │ considered"              │   └───────────────────────┘
        └───────────────────┘                            └───────────────────────┘
```

### Candidate gathering

The router queries the Capability Registry maintained by
[24 — Plugin Framework](24-plugin-framework.md) for every registered `ImplementationDescriptor`
whose `capability_id` matches the invocation. Registration, versioning, and how competing Plugins
come to offer the same Capability are that document's concern; this router only ever consumes the
registry, it never mutates it.

### The privacy gate (non-negotiable)

Before any candidate is scored, it is filtered by the privacy tier of the Context Bundle's data
classification, evaluated against [16 — Privacy Architecture](16-privacy-architecture.md)'s policy
for the user, device, and data class in play. This is a **gate, not a score component**: a cloud
candidate whose privacy tier exceeds what the bundle permits is removed from the candidate set
entirely, unconditionally, before scoring runs. There is no weight vector that can be tuned to
rescue it. This is the literal mechanism behind Design Invariant 3 — "never silently escalates to
cloud" is true only because the escalation path does not exist in the candidate list, not because
a score happens to disfavor it. A cloud candidate re-enters the set only when the bundle carries an
explicit, current, per-invocation-or-persistent consent record from 16, and even then it is scored
*lower* than an equally-qualified local candidate purely for being remote, so that local-first
holds as a preference even where cloud is legally admissible.

### Ensemble / verification pattern for high-stakes Capabilities

Some Capability contracts declare a **consequence tier** (irreversible, financial, medical, legal,
or otherwise high-stakes) — an *action-severity* axis, feeding directly into
[15 — Security Architecture](15-security-architecture.md)'s Risk Assessment Engine as one input to
its `sensitivity_score`. This is a genuinely different axis from 15's `provenance_tier` (who
published and vetted the code, which drives Trust Boundary/sandbox depth per
[03 — Kernel Architecture](03-kernel-architecture.md)) — a first-party, fully-trusted Capability can
still carry a `HighStakes` consequence tier, and a low-provenance community Capability can be purely
`Routine`. For consequence-tiered invocations, and for any invocation where the primary
implementation's own returned confidence falls below its declared threshold, the router dispatches
the same input to a second, architecturally diverse implementation (e.g., a different model family
or a cloud arbiter with different training data) in parallel rather than in sequence, and
reconciles:

- **Agreement** (semantic similarity of the two outputs above a contract-defined threshold):
  return the primary's output with confidence boosted, at added latency and cost.
- **Disagreement**: either invoke a third, designated tie-breaker implementation, or — per
  [01 §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable) — surface both outputs to
  the user through [18 — Explainability & Trust](18-explainability-and-trust.md) as the
  "alternatives considered" and pause for confirmation rather than silently picking one. Ensemble
  disagreement is never resolved by a coin flip.

### Registration and benchmarking over time

The benchmark table the scoring engine reads from is not hand-maintained. When
[24 — Plugin Framework](24-plugin-framework.md) registers a new `ImplementationDescriptor`, it
enters a staged rollout gated by [32 — Update System](32-update-system.md): **shadow** (receives
duplicate traffic, scored internally, never returned to a user) → **canary** (a small percentage of
live invocations, with the existing fallback chain still live as a safety net) → **general
availability** (fully eligible in scoring). Shadow traffic is duplicated *after* the same
`privacy_gate` check every live candidate passes (§Pseudocode `shadow_evaluate`), never before it —
"never returned to a user" bounds what the shadow candidate's output is used for, not whether it
was permissible to send it real Context Bundle data in the first place; an unvetted, newly
registered implementation (including a brand-new `CloudAPI`-kind one) that fails the privacy gate
never receives shadow traffic at all, exactly as if it were being routed live. A background
evaluation harness runs standard task
suites per Capability class against every registered implementation on a scheduled cadence and on
every new registration; field telemetry (latency, error rate, ensemble-disagreement rate, and
implicit signals such as how often a user edits or discards an implementation's output) continually
refreshes the same table. A regression in an implementation's field quality demotes it in future
scoring automatically, without a human curator in the loop for the common case.

## Data Structures

```rust
struct CapabilityInvocation {
    capability_id: CapabilityId,
    contract: SemanticContract,        // inputs, outputs, side effects — per 02 §Capability
    context_bundle: ContextBundleRef,  // 06, 07
    trust_boundary: TrustBoundaryId,   // 03
    urgency_class: Urgency,            // Interactive | Background | Batch
    consequence_tier: ConsequenceTier, // action-severity axis, declared below — feeds
                                        // 15-security-architecture.md's RiskAssessment as one
                                        // input to sensitivity_score (§Ensemble/verification)
    quality_floor: Option<f32>,        // caller-declared minimum acceptable confidence
    latency_budget: Duration,          // from 04-scheduler.md's admission model
}

/// Action-severity classification a Capability contract declares (independent of
/// 15-security-architecture.md's provenance_tier, which classifies code, not actions — see
/// §"Ensemble / verification pattern"). Declared here because this document is what actually
/// consumes it (routing to ensemble verification); 15 only reads it as one RiskAssessment input.
enum ConsequenceTier { Routine, Sensitive, HighStakes }

/// A single ImplementationDescriptor's live routing eligibility, distinct from
/// 32-update-system.md's `RolloutState` (which tracks a *version update's* fetch/stage/canary
/// progression as a whole). An implementation's `RolloutStage` advances as a *consequence* of
/// its underlying version record reaching the matching `RolloutState` in 32, but the two are
/// different enums for different questions: 32 asks "is this version safe to have installed at
/// all," this asks "should this specific implementation receive traffic right now."
enum RolloutStage { Shadow, Canary(f32), GA }

struct ImplementationDescriptor {
    impl_id: ImplId,
    capability_id: CapabilityId,
    kind: ImplKind,                 // LocalSmallModel | LocalLargeModel | CloudAPI | NativeBinary | Composed
    model_class: Option<ModelClass>, // SLM | LRM | Vision | Speech | Coding | Planning — 22;
                                      // orthogonal to `kind`: `kind` says size/locality tier,
                                      // `model_class` says functional family. None for
                                      // NativeBinary/Composed implementations with no model
                                      // backing them. This is the field 22's runtime.estimate()
                                      // is called with once this descriptor is selected.
    privacy_tier: PrivacyTier,      // 16
    resource_profile: ResourceProfile, // declared shape; resolved to a ResourceVector — 04
    cost_model: CostModel,          // Free | PerCall(f64) | PerToken(f64)
    quality_profile: BenchmarkTable, // task_class -> (quality, latency_p50, latency_p95)
    rollout_stage: RolloutStage,    // declared above; advanced by 32 as its RolloutState progresses
    trust_level: TrustLevel,        // 03 admission input
    owning_plugin: PluginId,        // 24
}

struct RoutingScore {
    impl_id: ImplId,
    latency_fit: f32,
    privacy_fit: f32,      // soft preference among admissible candidates only
    cost_fit: f32,
    quality_fit: f32,
    availability_fit: f32,
    composite: f32,
}

struct RoutingDecision {
    invocation_id: InvocationId,
    chosen: ImplId,
    fallback_chain: Vec<ImplId>,
    ensemble_group: Option<Vec<ImplId>>,
    rationale: Rationale,          // consumed by 18
    policy_version: PolicyVersion, // which weight table / benchmark snapshot was used
}

struct Rationale {
    candidates_considered: Vec<(ImplId, RoutingScore)>,
    candidates_excluded: Vec<(ImplId, ExclusionReason)>, // e.g. PrivacyGate, ResourceInfeasible
    chosen_reason: String,
}
```

## Algorithms

**1. Candidate gathering.** Query the registry for `ImplementationDescriptor`s matching
`capability_id`, restricted to `rollout_stage != Shadow` (shadow candidates are scored for
telemetry only, per §Architecture, and never chosen).

**2. Feasibility and privacy filtering.** Two hard gates, applied before any scoring: (a) the
privacy gate described above, consulting [16 — Privacy Architecture](16-privacy-architecture.md);
(b) a resource feasibility gate consulting [04 — Scheduler](04-scheduler.md) (current battery,
thermal state, CPU/GPU headroom) and [22 — Local AI Runtime](22-local-ai-runtime.md) (current VRAM
residency — is the candidate model already resident, or would it need a load/evict cycle whose
latency must be charged against `latency_budget`). A candidate that fails either gate is recorded
in `candidates_excluded` with a reason, for auditability, and never scored.

**3. Scoring.** For each surviving candidate, compute a weighted composite:

```
composite(c) = w_lat · latency_fit(c) + w_priv · privacy_fit(c)
             + w_cost · cost_fit(c)   + w_qual · quality_fit(c)
             + w_avail · availability_fit(c)
```

`latency_fit` is the candidate's benchmarked p95 latency for the invocation's task class,
normalized against `latency_budget` (1.0 at or under budget, decaying sharply past it).
`privacy_fit` rewards fully local execution over consented-cloud execution even after the hard gate
has passed, keeping local-first a live preference, not just a floor. `quality_fit` reads the
task-class-matched entry from `quality_profile`, computed by the benchmark harness described above.
The weight vector `(w_lat, w_priv, w_cost, w_qual, w_avail)` is selected by `urgency_class` and
`consequence_tier` — an `Interactive` UI-blocking call weights latency heavily; a `Batch` job
weights quality and cost; any `HighStakes` call floors `w_qual` regardless of urgency and disables
`cost_fit` from ever excluding a candidate outright.

**4. Fallback chain construction.** Sort surviving candidates descending by `composite`. The
ordered list *is* the fallback chain; the Scheduler walks it on timeout, error, or mid-execution
resource loss (e.g., a thermal event evicting a resident model), rather than re-invoking the router
from scratch each time, which would risk re-selecting an implementation that just failed.

**5. Ensemble trigger.** If `consequence_tier == HighStakes`, or the top candidate's own returned
confidence is below its contract's declared threshold, the top two *architecturally distinct*
candidates (preferring different `ImplKind` or different underlying model family, not just
different quantizations of the same model) are dispatched in parallel and reconciled per
§Architecture.

**6. Registration and staged promotion.** New `ImplementationDescriptor`s enter at `Shadow`;
promotion to `Canary` and `GA` is a [32 — Update System](32-update-system.md) decision gated on
benchmark-harness results and, once live, on field telemetry crossing quality/error thresholds —
never a router-internal decision, since promotion is a system update with its own rollback
requirements ([33 — Rollback & Recovery](33-rollback-recovery.md)).

## Interfaces / APIs

```
route(invocation: CapabilityInvocation) -> RoutingDecision
route_ensemble(invocation: CapabilityInvocation, k: usize) -> EnsembleDecision
report_outcome(impl_id: ImplId, invocation_id: InvocationId, outcome: Outcome) -> ()
get_rationale(decision_id: InvocationId) -> Rationale               // consumed by 18
register_implementation(desc: ImplementationDescriptor) -> RegistrationReceipt  // called by 24
set_rollout_stage(impl_id: ImplId, stage: RolloutStage) -> ()       // called by 32
```

`report_outcome` is how the field-telemetry half of §Architecture's benchmarking loop is fed; every
Capability invocation reports back latency, success/failure, and, where available, an implicit
quality signal (user edit/discard rate on the output), regardless of whether the invocation was
ordinary or ensemble-reconciled.

## Pseudocode

```rust
fn route(inv: &CapabilityInvocation, registry: &CapabilityRegistry) -> RoutingDecision {
    let mut considered = Vec::new();
    let mut excluded = Vec::new();

    let candidates = registry.lookup(inv.capability_id)
        .filter(|d| d.rollout_stage != RolloutStage::Shadow);
    shadow_evaluate(inv, registry);   // duplicate to Shadow candidates, privacy-gated identically

    for cand in candidates {
        if let Err(reason) = privacy_gate(&cand, &inv.context_bundle) {       // -> 16
            excluded.push((cand.impl_id, reason));
            continue;
        }
        if let Err(reason) = feasibility_gate(&cand, inv.latency_budget) {    // -> 04, 22
            excluded.push((cand.impl_id, reason));
            continue;
        }
        let w = weight_vector(inv.urgency_class, inv.consequence_tier);
        let score = RoutingScore {
            impl_id: cand.impl_id,
            latency_fit: latency_fit(&cand, inv.latency_budget),
            privacy_fit: privacy_fit(&cand),
            cost_fit: cost_fit(&cand, inv.consequence_tier),
            quality_fit: quality_fit(&cand, &inv.contract.task_class()),
            availability_fit: availability_fit(&cand),
            composite: 0.0, // filled below
        };
        let composite = w.lat * score.latency_fit + w.priv * score.privacy_fit
                       + w.cost * score.cost_fit   + w.qual * score.quality_fit
                       + w.avail * score.availability_fit;
        considered.push((cand.impl_id, RoutingScore { composite, ..score }));
    }

    considered.sort_by(|a, b| b.1.composite.partial_cmp(&a.1.composite).unwrap());
    if considered.is_empty() {
        return degrade_gracefully(inv, &excluded);   // 02 §4 invariant 5 — never fail closed
    }

    let fallback_chain: Vec<ImplId> = considered.iter().map(|(id, _)| *id).collect();
    let ensemble_group = if needs_verification(inv, &considered) {
        Some(pick_diverse_pair(&considered))
    } else {
        None
    };

    RoutingDecision {
        invocation_id: inv.id(),
        chosen: fallback_chain[0],
        fallback_chain,
        ensemble_group,
        rationale: Rationale { candidates_considered: considered, candidates_excluded: excluded,
                                chosen_reason: explain_top_choice() },
        policy_version: current_policy_version(),
    }
}

// Shadow-stage candidates are excluded from `route()`'s returned decision, but per
// §"Registration and benchmarking over time" they still receive duplicated real traffic for
// internal benchmarking — see 32-update-system.md and 35-testing-strategy.md. That
// duplication must pass through the identical privacy_gate a live candidate would, or a
// brand-new, unvetted (possibly CloudAPI-kind) implementation could receive real, sensitive
// Context Bundle data with no consent/residency check at all — a Design Invariant 3
// violation regardless of whether the output is ever returned to a user.
fn shadow_evaluate(inv: &CapabilityInvocation, registry: &CapabilityRegistry) {
    let shadow_candidates = registry.lookup(inv.capability_id)
        .filter(|d| d.rollout_stage == RolloutStage::Shadow);

    for cand in shadow_candidates {
        if privacy_gate(&cand, &inv.context_bundle).is_err() {   // -> 16, same gate as route()
            continue;   // never dispatched, never scored — not merely "not returned to the user"
        }
        dispatch_shadow_traffic(cand.impl_id, inv);   // scored internally per 32; result discarded
    }
}

fn reconcile_ensemble(a: Output, b: Output, contract: &SemanticContract) -> Reconciliation {
    if semantic_similarity(&a, &b) >= contract.agreement_threshold {
        Reconciliation::Agreed { output: a, confidence: boosted(a.confidence, b.confidence) }
    } else if let Some(tiebreaker) = contract.designated_tiebreaker {
        Reconciliation::TieBreak { output: invoke(tiebreaker, a.input.clone()) }
    } else {
        Reconciliation::EscalateToHuman { alternatives: vec![a, b] }  // -> 18, 01 §9
    }
}
```

## Security Considerations

The privacy gate is enforced structurally, not by convention: cloud candidates are removed from the
candidate *set*, so no scoring bug, weight misconfiguration, or malicious plugin-reported score can
resurrect them without an explicit consent record from [16 — Privacy Architecture](16-privacy-architecture.md).
Quality scores are never self-reported by a Plugin; they come only from the independent benchmark
harness and field telemetry, closing the obvious downgrade attack where a low-quality implementation
claims a high score to win routing traffic. Every `RoutingDecision` is recorded immutably for
[34 — Observability & Telemetry](34-observability-telemetry.md) and surfaced on demand via
[18 — Explainability & Trust](18-explainability-and-trust.md); a rationale is built only from data
already visible inside the invoking Trust Boundary, so explaining a decision never leaks
information across a [Trust Boundary](02-core-architecture.md#trust-boundary) the invocation itself
didn't cross. Dispatch of the chosen implementation still passes through the kernel's
`cap_invoke` gate (see [03 — Kernel Architecture](03-kernel-architecture.md#interfaces--apis)) — the
router *chooses*, it never itself holds ambient authority to *execute*.

## Failure Modes

- **Chosen implementation times out or errors.** The Scheduler advances to the next entry in
  `fallback_chain` without re-invoking the router, preserving the Context Bundle across the switch.
- **All candidates infeasible** (e.g., offline with no privacy-admissible local model resident).
  `degrade_gracefully` follows Design Invariant 5: offer a reduced-scope Capability, queue for
  later, or ask the user, rather than failing the Intent silently.
- **Ensemble disagreement with no designated tie-breaker.** Escalates to the human per
  [01 §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable); never resolved
  automatically.
- **Benchmark table staleness.** A quality regression introduced by an upstream model update is
  invisible until field telemetry accumulates; mitigated by canary-stage exposure limits from
  [32 — Update System](32-update-system.md).
- **Registry/router disagreement** (a Plugin unregistered mid-invocation). Treated identically to a
  timeout — advance the fallback chain.

## Recovery Mechanisms

Each `ImplementationDescriptor` carries a circuit breaker: after *n* consecutive failures it is
temporarily demoted to the bottom of any fallback chain (not removed — it can recover) until a
cooldown and a successful health probe restore it. A bad canary promotion is rolled back through
[32 — Update System](32-update-system.md) and [33 — Rollback & Recovery](33-rollback-recovery.md)
exactly like any other staged rollout, reverting affected `rollout_stage` values without requiring
Capability callers to change anything. Because the fallback chain is precomputed at routing time,
recovery from a mid-chain failure costs one hop, not a full re-route.

## Performance Analysis

The routing decision itself must be a small fraction of the invocation's own latency budget —
candidate lists are cached per `capability_id` and invalidated only on registry change (from
[24 — Plugin Framework](24-plugin-framework.md)) or rollout-stage change (from
[32 — Update System](32-update-system.md)), so the common-case `route()` call is a cache read plus
a fixed-size weighted sum, targeting low-single-digit milliseconds — negligible against the
sub-second workspace generation target in [36 — Performance Benchmarks](36-performance-benchmarks.md).
The ensemble pattern roughly doubles latency and cost for the invocations it applies to; it is
scoped deliberately to `HighStakes` and low-confidence cases so it never taxes the common path.

## Trade-offs

A transparent, weighted-sum scoring function is less optimal than a learned ranking policy (a
contextual bandit or small RL policy could plausibly route better over time) but is directly
inspectable — every term in `composite()` is nameable in a `Rationale` — which
[18 — Explainability & Trust](18-explainability-and-trust.md) requires. Hyperion accepts a ceiling
on routing sophistication in exchange for keeping "why did it pick this" answerable in plain
language; a future learned component may adjust the weight vector itself, but the scored terms and
their evaluation must remain individually explainable, not a single opaque score. Ensemble
verification trades cost and latency for correctness assurance on a narrow, deliberately chosen
slice of invocations rather than universally, which accepts residual risk on Capability calls whose
consequence tier is under-declared by their Plugin — a gap [15 — Security Architecture](15-security-architecture.md)
and [17 — Threat Model](17-threat-model.md) must also defend against independently.

## Testing Strategy

Golden task suites per Capability class are run against every registered implementation before
`Canary` promotion, and continuously in `Shadow` for anything already registered. Fuzz testing
specifically targets the privacy gate: adversarial invocations attempt to get a cloud candidate
selected for a locally-classified Context Bundle by manipulating urgency, cost, or quality inputs —
this must fail unconditionally regardless of score manipulation. Chaos tests kill the chosen
implementation mid-invocation and verify the fallback chain completes within budget. Rationale
generation is regression-tested independently, since a routing decision that is correct but
unexplainable fails [18 — Explainability & Trust](18-explainability-and-trust.md) as surely as a
wrong one.

---
*Next: [24 — Plugin Framework](24-plugin-framework.md).*
