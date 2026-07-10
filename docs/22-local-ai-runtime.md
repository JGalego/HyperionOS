# Local AI Runtime

This document specifies Hyperion's on-device inference runtime: the component that loads model
weights and executes forward passes on whatever compute a device actually has, for every model
class the rest of the system depends on — small language models for always-on intent interpretation,
large reasoning models, vision, speech, coding, and planning models. It sits directly on the
Compute device class defined by
[03 — Kernel Architecture](03-kernel-architecture.md#hardware-abstraction-layer)'s HAL and is
scheduled by the same unified ledger as every other workload in
[04 — Scheduler](04-scheduler.md) — it does not run a second, competing resource-management loop.
It answers exactly one question, for a model that has already been chosen: **how does this run, on
this hardware, right now?**

## Purpose

Own model lifecycle management (which model tiers sit resident in RAM/VRAM versus load on demand),
hardware-adaptive execution (which quantization/precision variant runs, chosen from what the device's
HAL Compute contract actually advertises), and power/thermal-aware inference scheduling, so that the
same [Capability](02-core-architecture.md#capability) degrades gracefully from an enterprise GPU
workstation down to a Raspberry-Pi-class device per
[01 §10](01-vision-and-philosophy.md#10-success-criteria), without the Capability author or the user
ever specifying hardware details.

### Scope Boundary

This is the single most important thing this document must state precisely, because
[23 — Multi-Model Orchestration](23-multi-model-orchestration.md) sits immediately above it and the
two are easy to conflate:

| Question | Owner |
|---|---|
| Which model/implementation satisfies this Capability — a local SLM, a local LRM, a cloud API? | [23 — Multi-Model Orchestration](23-multi-model-orchestration.md) |
| Should this Capability's execution be allowed to leave the device at all? | [16 — Privacy Architecture](16-privacy-architecture.md) decides the policy; [21 — Distributed Execution](21-distributed-execution.md) is the mechanism if it may |
| Given "run local model M, class C," at what precision and residency state does it execute on *this* hardware, *right now*? | **This document** |
| How much VRAM / inference-token budget does this task get relative to everything else running on this device? | [04 — Scheduler](04-scheduler.md) admits and dispatches; this document supplies the candidate `ResourceVector`s it chooses from |

Concretely: 23's Model Router calls `runtime.estimate()` (§Interfaces) to learn what running a given
`ModelClass` locally would cost and how fast it would go, on *this* device, at *this* moment; 23
decides, in consultation with [16](16-privacy-architecture.md)'s consent gate, whether that local
option or a remote one wins. Only after that decision lands on "run it here" does this document do
any actual work. This document never independently decides to route a Capability to the cloud.

## Motivation

Three technology curves in [01 §3](01-vision-and-philosophy.md#3-why-now) converge to make an
intent-native OS possible, and the first is "local-capable reasoning models small and efficient
enough to run continuously on consumer hardware... making always-on intent interpretation affordable
in latency, cost, and power." That sentence is a runtime requirement, not a modeling one: an
always-on SLM interpreting every utterance must never become a felt battery or heat tax, which is
exactly the brief's demand for "power/thermal-aware inference scheduling for battery life." Design
Invariant 3 ([02 §4](02-core-architecture.md#4-design-invariants), local-first by default) makes
local execution the default path for every model class this document manages, not an optimization
applied opportunistically. And [37 — Scalability Roadmap](37-scalability-roadmap.md)'s hardware range
— Raspberry Pi-class SBCs through enterprise GPU clusters — means the *same* Capability contract must
resolve to a viable execution plan on both ends without a developer writing hardware-specific code;
that resolution is this document's central algorithm (§5.1).

Finally, [04 — Scheduler §Motivation](04-scheduler.md#motivation) is explicit that a scheduler
which reasons about classical and AI resources as two separate, bolted-on systems will let them
silently starve each other. This document takes that warning seriously in its own architecture: it
deliberately does not introduce a second thermal/battery control loop alongside 04's — see
§Architecture and §5.3.

## Architecture

```
   ┌──────────────────────────────────────────────────────────────┐
   │       23 — Multi-Model Orchestration (Model Router)           │
   │   "run Capability X using local model M, class C" (decided)   │
   └────────────────────────────────┬───────────────────────────-─┘
                runtime.estimate()   │   runtime.infer()
   ┌────────────────────────────────▼─────────────────────────────┐
   │                LOCAL AI RUNTIME (this document)                │
   │                                                                 │
   │   Model Registry       Residency Manager     Power/Thermal      │
   │   (descriptors +  ───▶ (hot/warm/cold,  ───▶  Policy (§5.3 —    │
   │    variants)             §5.2)                 consumes 04's    │
   │                                                 governor tick)  │
   │                                │                                │
   │                                ▼                                │
   │       Execution Engine — one KV-cache/context per Trust         │
   │       Boundary; batches within a boundary, never across it      │
   └────────────────────────────────┬────────────────────────────-─┘
                                     │ queue submission
   ┌────────────────────────────────▼─────────────────────────────┐
   │  03 — Kernel HAL: Compute device class (CPU SIMD · GPU · NPU)  │
   │  capacity descriptor: cores/SMs/TOPS; queue + preemption model │
   └────────────────────────────────────────────────────────────-──┘
                  ▲                                      ▲
                  │ admission / dispatch                 │ throttle factor
   ┌───────────────┴──────────────────────────────────────┴───────┐
   │        04 — Scheduler: ResourceVector (vram_mb,                │
   │  inference_tokens_per_sec, context_window_slots, battery_mw)   │
   │        + Thermal/Battery Feedback Governor                     │
   └────────────────────────────────────────────────────────────-──┘
```

Model weights themselves are not managed by a bespoke filesystem: they are content-defined-chunked
blobs in [28 — Storage Engine](28-storage-engine.md#data-structures) ("large objects — photos,
videos, local model weights — are content-defined-chunked so that a small edit ... only writes the
changed chunks"), fetched via `get_object`, signature-verified, and versioned exactly like any other
large Semantic Object. Model updates arrive as new blob versions through
[32 — Update System](32-update-system.md)'s signed package pipeline; this document never fetches raw
weights from an unauthenticated source. This places the runtime at the boundary between L1 System
Runtime (it is a scheduled consumer of physical GPU/NPU dispatch, per
[02 §1](02-core-architecture.md#1-layered-system-view)) and L4 Cognition Layer (it is invoked by the
Model Router and, indirectly, by any Agent making a Capability call that resolves to local inference).

## Data Structures

```rust
enum ModelClass { SLM, LRM, Vision, Speech, Coding, Planning }   // functional family; 23 selects
                                                                    // one, this doc shapes execution
                                                                    // around its resource/streaming
                                                                    // shape (e.g. speech needs a
                                                                    // streaming audio I/O path; vision
                                                                    // needs image-tensor batching)

struct ModelDescriptor {
    model_id: ObjectId,                    // resolved via 28-storage-engine.md's get_object (CABS blob)
    class: ModelClass,
    param_count: u64,
    variants: Vec<QuantizedVariant>,
    signature: Signature,                  // verified per 15-security-architecture.md before load
}

struct QuantizedVariant {
    precision: Precision,                  // FP16 | INT8 | INT4 | GGUF_Qn
    footprint_mb: u32,
    min_compute_class: CapacityDescriptor, // 03-kernel-architecture.md HAL Compute contract
    expected_tokens_per_sec: Map<HardwareProfile, f32>,
}

struct ResidencyEntry {
    model_id: ObjectId,
    status: Hot | Warm | Cold,             // in-VRAM-ready | in-RAM-needs-transfer | on-disk
    last_used: Instant,
    pin_count: u32,                        // e.g. the always-on intent-interpretation SLM is pinned
    predicted_next_use: f32,               // from 06-context-engine.md's working-set signal
}

struct PowerBudget {
    mode: Performance | Balanced | BatterySaver | Critical,
    derived_from: ThrottleFactor,           // consumed, not computed — see 04's governor tick, §5.3
}

struct InferenceHandle {
    request_id: RequestId,
    model_id: ObjectId,
    stream: TokenStream,                    // cancellable — 01 §9 "interruptible"
    trust_boundary: TrustBoundaryId,        // 03; one per invocation, never shared across boundaries
}
```

## Algorithms

**5.1 Hardware-adaptive tier selection.** Given a `ModelClass` and the device's HAL-reported
`CapacityDescriptor` (03), select the highest-quality `QuantizedVariant` whose `footprint_mb` fits
within the `vram_mb`/`ram_mb` [04 — Scheduler](04-scheduler.md) is prepared to grant for this class
of task, and whose `expected_tokens_per_sec` for this hardware profile meets the Capability's
latency budget. On failure, step down one rung (lower precision, then a smaller variant of the same
class) and retry the fit check — this is the concrete mechanism behind
[02 §4](02-core-architecture.md#4-design-invariants)'s "degrade, never fail closed" for on-device
inference specifically, and it is what lets a Raspberry-Pi-class device run the same Capability an
enterprise GPU box runs, just at a visibly and honestly lower tier.

**5.2 Residency / working-set management.** A page-replacement-style algorithm: evict the least
valuable `Cold` candidate when a newly needed model does not fit within the capacity 04's
`query_ledger` currently reports as available. Value is `recency * predicted_next_use`, with
`pin_count` overriding eviction entirely for models an operator has pinned (the always-on
intent-interpretation SLM is permanently pinned so it is never a candidate for eviction under normal
operation). `predicted_next_use` is read, not computed independently — it is
[06 — Context Engine](06-context-engine.md#data-structures)'s working-set signal, so residency
decisions track the same session context the rest of the cognition layer already maintains.

**5.3 Power/thermal-aware inference policy.** This document deliberately does not sample battery
or thermal sensors itself, or run a second PI control loop alongside
[04 — Scheduler §Algorithms 3](04-scheduler.md#algorithms)'s governor — doing so is precisely the
"two subsystems silently starve each other" failure 04's own Motivation warns against. Instead, it
subscribes to the scaled `ResourceLedger.capacity` 04's governor already produces for the
`vram_mb`/`inference_tokens_per_sec` dimensions. A sustained scale-down is mapped to a `PowerBudget`
mode, which gates three decisions that only this document — not 04 — has enough model-level
information to make: how many concurrent inference streams may run, whether the currently resident
`QuantizedVariant` must be swapped for a smaller one, and whether speculative preloading (§5.4) is
permitted at all.

**5.4 Predictive warm-start.** [06 — Context Engine](06-context-engine.md)'s working-set signal
predicts the next likely Capability invocation; the Residency Manager opportunistically promotes a
`Cold` model to `Warm` or `Hot` during idle cycles. This only ever runs under `Performance` or
`Balanced` `PowerBudget` mode — never `BatterySaver` or `Critical` — so a snappier next interaction
is never purchased with battery the user did not agree to spend.

## Interfaces / APIs

```
runtime.estimate(model_class, capability_contract) -> [CandidateResourceVector]  // consumed by 23/04
runtime.load(model_id, variant_hint?) -> ModelHandle
runtime.infer(handle, request) -> InferenceHandle             // cancellable stream, per 01 §9
runtime.cancel(request_id)
runtime.residency.pin(model_id) / .unpin(model_id)
runtime.power.currentMode() -> PowerBudget
```

Events published on [31 — Event System](31-event-system.md): `model.loaded`, `model.evicted`,
`inference.started/completed/cancelled/throttled`, `power.mode_changed`. `runtime.estimate` is the
narrow feedback interface referenced in §Scope Boundary: it returns one or more candidate
`ResourceVector`s (04's struct, unmodified) per feasible `QuantizedVariant`, which is exactly what
23's Model Router needs to compare a local option against a remote one, and exactly what 04's
"model-tier coupling" admission step (04 §Algorithms 4) retries against in priority order.

## Pseudocode

```python
def select_variant(model_class, capability_contract, hardware_profile, ledger_snapshot):
    descriptor = model_registry.lookup(model_class, capability_contract.preferred_model)
    for variant in sorted(descriptor.variants, key=lambda v: v.precision, reverse=True):  # best first
        if variant.footprint_mb > ledger_snapshot.available(hardware_profile.vram_dim):
            continue                                            # doesn't fit this device right now
        tokens_per_sec = variant.expected_tokens_per_sec.get(hardware_profile)
        if tokens_per_sec is None or not meets_latency_budget(tokens_per_sec, capability_contract):
            continue                                            # too slow for this task's deadline
        return variant, to_resource_vector(variant, hardware_profile)     # §5.1 success

    return None, None            # no local variant fits at all — signal infeasible to 23/21


def load_and_infer(model_class, capability_contract, request):
    power = runtime.power.currentMode()
    variant, vector = select_variant(model_class, capability_contract,
                                      hal.compute_profile(), scheduler.query_ledger())
    if variant is None:
        return InferenceResult.infeasible_locally()             # 23 may now consider remote/cloud

    handle = residency.ensure_loaded(model_class, variant, pin=capability_contract.always_on)
    if power.mode in (BatterySaver, Critical):
        handle = residency.downgrade_if_needed(handle, power)   # §5.3 — never silently ignored

    ticket = scheduler.submit_task(TaskDescriptor(               # 04-scheduler.md
        request=vector, model_tier_hint=None,                    # already resolved to a concrete vector
        cap_token=request.cap_token, class_=SchedClass.InteractiveAgent))

    stream = execution_engine.run(handle, request, trust_boundary=request.trust_boundary)
    events.emit("inference.started", handle.model_id, request.request_id)
    return InferenceHandle(request.request_id, handle.model_id, stream, request.trust_boundary)
```

## Security Considerations

Every model artifact is signature-verified (per
[15 — Security Architecture](15-security-architecture.md)) before load, and its bytes are
self-verifying by content hash on every read because they live in
[28 — Storage Engine](28-storage-engine.md#recovery-mechanisms)'s content-addressed blob store —
silent weight corruption or tampering is detected, not served. The Execution Engine enforces
exactly one KV-cache/context per Trust Boundary: it may batch multiple concurrent requests from the
same Agent's own invocations for throughput, but it never batches across two different Trust
Boundaries, so no Agent's prompt or generated content is ever physically adjacent, in the same
batch, to another Agent's — a hardware efficiency optimization is never allowed to become a
cross-boundary information leak. The inference process itself is sandboxed as its own Trust Boundary
(03's sandboxing spectrum, typically depth 1, promoted to depth 2/3 for untrusted third-party model
plugins registered via [24 — Plugin Framework](24-plugin-framework.md)), so a malformed or malicious
model artifact cannot escalate beyond the capability-scoped resources it was granted. Shared physical
hardware — most acutely a home-server-class device serving several federated devices' offloaded
inference per [21 — Distributed Execution](21-distributed-execution.md) — introduces a residual
timing/power side-channel risk across Trust Boundaries; this is mitigated with constant-shape
batching windows and cache partitioning where the HAL (03) exposes it, and the remaining residual
risk is catalogued, not hand-waved, in [17 — Threat Model](17-threat-model.md). Telemetry sent to
[34 — Observability & Telemetry](34-observability-telemetry.md) excludes prompt and response content
by default, consistent with [16 — Privacy Architecture](16-privacy-architecture.md).

## Failure Modes

- **Out of memory** — no `QuantizedVariant` fits even at the lowest precision for this device class.
- **Thermal throttling mid-generation** — the governor's scaled capacity drops below what the
  currently-running stream was admitted against.
- **Driver crash** — the NPU/GPU driver underneath the Execution Engine faults mid-inference.
- **Corrupted or unverified model artifact** — signature check fails, or content-hash mismatch is
  detected on read.
- **Contention starvation** — many Agents request inference concurrently and one is starved of
  scheduler time.
- **Critical battery mid-generation** — the device must cut inference power before a stream
  completes.

## Recovery Mechanisms

Tier downgrade (§5.1) is the first line of defense against out-of-memory and thermal throttling: it
never fails closed, and if truly nothing fits locally, `load_and_infer` returns an explicit
`infeasible_locally` result so [23](23-multi-model-orchestration.md) and
[21](21-distributed-execution.md) can consider a federation device or a consented cloud path, rather
than the caller silently getting nothing. A driver crash triggers the same supervised-restart
("microreboot") pattern [03 — Kernel Architecture](03-kernel-architecture.md#recovery-mechanisms)
already defines for any sandboxed device driver; in-flight requests are retried transparently up to
a bounded count, after which the caller receives an explainable cancellation
([18](18-explainability-and-trust.md)), never a silent hang. A corrupted or unverified artifact is
rejected at load time and re-fetched through [32 — Update System](32-update-system.md)'s trusted
distribution path rather than served. Contention starvation is resolved entirely by
[04 — Scheduler](04-scheduler.md#algorithms)'s own fair-share dispatch and aging mechanism — this
document does not implement a second fairness policy. Critical battery mid-generation pauses rather
than kills a stream: partial output already delivered to the caller is preserved (a
[33 — Rollback & Recovery](33-rollback-recovery.md) checkpoint of the partial generation), and the
remainder resumes on charging or, with consent, is handed to another federation device through
[21 — Distributed Execution](21-distributed-execution.md).

## Performance Analysis

The always-on intent-interpretation SLM (§Motivation) is preloaded and pinned at boot specifically
to hit the sub-5-second cold-boot target in
[36 — Performance Benchmarks](36-performance-benchmarks.md); larger tiers load on demand. Illustrative
first-token latency budgets: an SLM class under 150 ms; a mid-tier coding or vision model under
400 ms; a large reasoning model in the one-to-three-second range, acceptable because the Dynamic UI
Runtime ([13](13-dynamic-ui-runtime.md)) can render an explicit "thinking" affordance for that class
rather than a blocking freeze. Small models are frequently memory-bandwidth-bound rather than
compute-bound on CPU-only hardware, which is why aggressive quantization (INT4/GGUF-class) buys a
disproportionate speedup for exactly the tier most likely to run on constrained hardware. On a
Raspberry-Pi-class device per [37 — Scalability Roadmap](37-scalability-roadmap.md), typically only
the SLM tier stays resident; requests for Vision, LRM, or Planning classes are transparently routed
through [21 — Distributed Execution](21-distributed-execution.md) to a federated or, with consent, a
cloud device — never silently disabled, consistent with "degrade, never fail closed."

## Trade-offs

Aggressive quantization improves residency footprint and speed at some cost to answer quality; this
is deliberately treated as a substitutable-implementation choice at the [23](23-multi-model-orchestration.md)
layer (a quantized local variant versus a larger remote one are two implementations of the same
Capability with different confidence) rather than something this document silently optimizes for
without surfacing the trade — [18 — Explainability & Trust](18-explainability-and-trust.md) can show
it when it matters. Predictive preloading (§5.4) improves felt responsiveness at a real battery and
thermal cost, which is why it is hard-gated by `PowerBudget` mode rather than always-on. A single
shared Execution Engine process with Trust-Boundary-partitioned batching windows is more efficient
than one sandboxed process per Agent, at a small residual side-channel cost (§Security
Considerations); a stricter "one inference sandbox per Agent" mode is available for high-sensitivity
Capabilities at a throughput cost, chosen per Capability trust tier by
[15 — Security Architecture](15-security-architecture.md), not globally.

## Testing Strategy

A hardware-matrix CI suite runs an identical Capability regression set across emulated profiles
spanning enterprise GPU, mid-tier consumer GPU, NPU-only laptop, CPU-only, and Raspberry-Pi-class
hardware, asserting every case degrades gracefully to a feasible tier and none fails closed. A
thermal/battery simulation harness drives the `PowerBudget` state machine through mode transitions
and asserts inference throttles, downgrades, or pauses and resumes correctly, with in-progress output
never silently dropped. Model-artifact fuzzing confirms corrupted or unsigned artifacts are always
rejected before load. Load tests run many concurrent Agents' inference requests to validate
[04](04-scheduler.md)'s fairness under contention and, independently, that no cross-Trust-Boundary
context or KV-cache bleed occurs under batching (a combined correctness-and-security test). Golden
quality-regression tests bound the answer-quality delta across quantization tiers for representative
Capabilities. A multi-hour soak test on battery-class hardware under continuous inference confirms
no thermal runaway and correct governor-following behavior over time, feeding
[35 — Testing Strategy](35-testing-strategy.md).

---
*Next: [23 — Multi-Model Orchestration](23-multi-model-orchestration.md) picks up where this
document's `runtime.estimate()` interface leaves off — deciding, across every local and remote
implementation this and [21 — Distributed Execution](21-distributed-execution.md) can offer, which
one actually serves a given Capability.*
