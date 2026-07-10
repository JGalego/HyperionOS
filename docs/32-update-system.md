# Update System

## Purpose

This document specifies how Hyperion updates itself: the kernel/OS image
([03 — Kernel Architecture](03-kernel-architecture.md) and the L1 System Runtime), individual
Capability and Plugin implementations ([24 — Plugin Framework](24-plugin-framework.md)), and local
AI models ([22 — Local AI Runtime](22-local-ai-runtime.md),
[23 — Multi-Model Orchestration](23-multi-model-orchestration.md)). It defines the atomic
partition scheme for the system image, independent versioning and staged rollout for the other
two update classes, the compatibility checks — including database schema migrations against
[29 — Database Schema](29-database-schema.md) — that must pass before an update is applied, and
how every update integrates with its sibling document, [33 — Rollback & Recovery](33-rollback-recovery.md),
which is what actually makes an update reversible.

## Motivation

[02 — Core Architecture §4](02-core-architecture.md#4-design-invariants) invariant 2 requires that
*"everything is undoable or versioned"* and that *"state-changing operations produce a recovery
point before they execute."* An update — of the OS image, a Capability, or a model — is the single
highest-blast-radius state-changing operation Hyperion performs: it is one of the few actions that
can, if mishandled, leave the device unbootable or the user's [Knowledge Graph](09-knowledge-graph.md)
in a state no Agent or user can reason about. [01 — Vision & Philosophy §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable)
requires every autonomous action to be interruptible, undoable, auditable, and observable; an
update the user did not explicitly review line-by-line is exactly the kind of autonomous action
that principle exists to constrain. At the same time, [01 §10](01-vision-and-philosophy.md#10-success-criteria)
requires a cold boot under five seconds and near-instant wake on hardware ranging from
Raspberry Pi-class devices to enterprise clusters ([37 — Scalability Roadmap](37-scalability-roadmap.md)),
so an update mechanism that requires long offline maintenance windows is disqualified on
usability grounds before it is even disqualified on safety grounds. Finally, because Hyperion
replaces the application with the [Capability](02-core-architecture.md#capability) as the unit of
installable software, an update system that can only ship whole-OS images would force every
third-party Capability fix or model refresh through a full system release cycle — this is
incompatible with an ecosystem of independently evolving Capabilities and models
([24 — Plugin Framework](24-plugin-framework.md), [25 — SDK](25-sdk.md)).

## Architecture

Hyperion treats "update" as three independent tracks that share one orchestrator, one
compatibility gate, and one rollback substrate — never three unrelated update mechanisms. A bad
Capability update must never require reverting the kernel; a bad model refresh must never require
reinstalling a Capability; a bad kernel update must never silently corrupt Capability or model
state it did not intend to touch.

```
                         ┌──────────────────────────────────┐
                         │   Update Orchestrator  (L2)        │
                         │   Platform Services layer           │
                         └────────────────┬─────────────────┘
                                          │ reads UpdateManifest, drives all tracks
      ┌───────────────────┬───────────────┼───────────────────┬────────────────────┐
      │                   │               │                   │                    │
┌─────▼───────┐   ┌───────▼────────┐ ┌────▼──────────┐  ┌─────▼─────────┐  ┌───────▼────────┐
│ System Image │   │ Capability /    │ │ Model          │  │ Compatibility  │  │ Rollout Cohort  │
│ Track        │   │ Plugin Track    │ │ Track          │  │ Checker        │  │ Controller      │
│ (A/B slots,  │   │ (24-*.md,       │ │ (22-*.md,      │  │ (schema diff,  │  │ (canary %,      │
│  03-*.md)    │   │  registry swap) │ │  23-*.md)      │  │  29-*.md)      │  │  34-*.md health)│
└─────┬────────┘   └───────┬─────────┘ └───────┬────────┘  └───────┬────────┘  └────────┬────────┘
      │                    │                   │                   │                    │
      │ boot slot flip     │ capability        │ router             │ migration plan     │ health-gated
      │ + boot counter     │ pointer swap       │ re-point + fallback│ (expand/contract)  │ percentage ramp
      ▼                    ▼                   ▼                   ▼                    ▼
┌────────────────────────────────────────────────────────────────────────────────────────────┐
│   33 — Rollback & Recovery: recovery point BEFORE apply · automatic rollback on gate failure  │
└────────────────────────────────────────────────────────────────────────────────────────────┘
```

### System Image Track

The OS image — the L0 privileged core, the L1 System Runtime, and the base set of
always-present platform services — is distributed and applied as an **atomic, dual-slot (A/B)
image**, the same pattern used for the sandboxing depths in
[03 — Kernel Architecture](03-kernel-architecture.md#sandboxing-as-one-spectrum): one slot is
always "active and known-good," the other is the write target for staging the next version. A
new image is written in full to the inactive slot while the active slot continues to serve the
running system — there is no offline maintenance window and no partially-written boot path ever
becomes the boot target. Activation is a single bootloader-owned pointer flip plus a boot-success
counter: the bootloader boots the newly-activated slot for up to *N* boot attempts, and only
commits to it permanently once the system runtime reports first-successful-boot health back to
the bootloader's early-boot record; failing that, the bootloader reverts the pointer to the
previous slot with zero user action required (§Recovery Mechanisms). This is a hardware-adjacent
analogue of the "recovery point before it executes" invariant: the prior slot *is* the recovery
point for the whole system image.

### Capability / Plugin Track

Each Capability version is a signed, content-addressed package in the
[Storage Engine](28-storage-engine.md), independent of the OS image's version and of every other
Capability's version, matching [02 — Core Architecture's](02-core-architecture.md#capability)
requirement that Capabilities be independently installable units. The Capability Registry
(L2, alongside the Plugin Framework) holds, per Capability, a currently-active version pointer and
a small history of prior versions retained for instant rollback. Updating a Capability never
touches the system image track: a new version is installed into its own Trust Boundary
(per [03's sandboxing depth table](03-kernel-architecture.md#sandboxing-as-one-spectrum)) alongside
the running version, health-checked, and only then does the registry pointer swap — a blue-green
deployment at Capability granularity. In-flight invocations of the old version are allowed to drain
under their existing capability tokens before those tokens are revoked
([03 §Algorithms](03-kernel-architecture.md#algorithms)), so an update never aborts a Capability
mid-Intent.

### Model Track

Local AI models ([22 — Local AI Runtime](22-local-ai-runtime.md)) are versioned exactly like
Capabilities but carry an additional compatibility dimension: a `ModelHardwareRequirement`
(quantization format, accelerator target — a property of the model build itself, distinct from
[37 — Scalability Roadmap](37-scalability-roadmap.md#data-structures)'s `HardwareProfile`, which
describes the *device's* tier the model would run on) and behavioral profile (benchmark score
deltas the [Model Router](23-multi-model-orchestration.md#architecture) uses for selection). A model update
never mutates the model a Capability is currently bound to mid-invocation; the Model Router
resolves a new model version only for new invocations once the new version has passed its
canary window, and retains the fallback chain to the prior model version so that "degrade, never
fail closed" ([02 §4.5](02-core-architecture.md#4-design-invariants)) holds even if a freshly
promoted model regresses in the field.

### Update Orchestrator, Compatibility Gate, and Rollout Cohort Controller

One Update Orchestrator (an L2 Platform Services component) drives all three tracks through the
same four-stage pipeline: **fetch → compatibility-check → stage → phased-apply**. The
Compatibility Checker inspects an `UpdateManifest` (§Data Structures) against the live system:
schema version range against [29 — Database Schema](29-database-schema.md), Capability contract
version against dependent Capabilities, model hardware profile against the local
[Device Framework](20-device-framework.md) descriptor. The Rollout Cohort Controller owns the
canary/phased policy (§Algorithms), consulting [34 — Observability & Telemetry](34-observability-telemetry.md)
health signals to gate percentage ramp-up, and reports every stage transition on the
[Event System](31-event-system.md) so the user's Explanation Record
([18 — Explainability & Trust](18-explainability-and-trust.md)) can answer "why is this updating,
and why now."

## Data Structures

```rust
enum UpdateSubject {
    SystemImage,
    Capability { id: CapabilityId },
    Model { id: ModelId },
}

/// What a specific model *build* itself requires — not to be confused with
/// 37-scalability-roadmap.md's `HardwareProfile`, which describes a *device's* tier
/// (SBC/Laptop/Workstation/EnterpriseNode). A device's `HardwareProfile` is what
/// `hardware_compatible` in `CompatibilityCheckResult` (below) checks this against.
struct ModelHardwareRequirement {
    quantization: QuantizationFormat,   // e.g. INT4, INT8, FP16 — see 22-local-ai-runtime.md
    accelerator_target: AcceleratorClass, // CPU | GPU | NPU — 03-kernel-architecture.md's HAL
    min_vram_mb: u32,
}

struct UpdateManifest {
    subject: UpdateSubject,
    from_version: SemVer,
    to_version: SemVer,
    package_ref: ContentHash,          // 28-storage-engine.md content-addressed blob
    signature: Signature,              // verified per 15-security-architecture.md
    schema_range: Option<SchemaRange>, // (min, max) against 29-database-schema.md
    migration_plan: Option<MigrationPlan>,
    model_hardware_requirement: Option<ModelHardwareRequirement>, // model track only —
                                        // deliberately NOT 37-scalability-roadmap.md's
                                        // HardwareProfile, which describes a *device's* tier;
                                        // this describes what a *model build* itself requires
    rollout_policy: RolloutPolicy,
}

struct RolloutPolicy {
    stages: Vec<CohortStage>,          // e.g. [1%, 10%, 50%, 100%]
    min_soak_time: Duration,           // per stage, before advancing
    health_thresholds: HealthThresholds, // crash rate, latency, error budget
    auto_rollback_on_breach: bool,
}

enum RolloutState {
    Fetched,
    CompatibilityChecked(CompatibilityCheckResult),
    Staged,
    Canary { stage_index: u8, started_at: Instant },
    RolledOut,
    RolledBack { reason: RollbackReason },
}

struct CompatibilityCheckResult {
    schema_compatible: bool,
    migration_required: bool,
    blocking_dependencies: Vec<CapabilityId>,
    hardware_compatible: bool,
}

struct SystemImageSlot {
    slot: Slot,                        // A | B
    version: SemVer,
    boot_attempts_remaining: u8,
    committed: bool,                   // set once first-boot health check passes
}

struct CapabilityVersionRecord {
    capability_id: CapabilityId,
    version: SemVer,
    package_hash: ContentHash,
    state: RolloutState,
    prior_version: Option<SemVer>,     // instant-rollback target
}
```

## Algorithms

**Staged / canary rollout.** Advancement through `RolloutPolicy.stages` is monotonic and
health-gated, never time-gated alone: at each stage, the Rollout Cohort Controller selects a
device cohort (by device tier, opt-in class, or random hash bucket of device ID) proportional to
the stage's percentage, waits at least `min_soak_time`, and only advances if
[34 — Observability & Telemetry](34-observability-telemetry.md) reports the update cohort's crash
rate, latency, and Capability-specific health metrics within `health_thresholds` relative to the
control cohort still on the old version. A breach at any stage halts the rollout and, if
`auto_rollback_on_breach` is set (the default for all system-image and most Capability updates),
triggers an automatic rollback through [33 — Rollback & Recovery](33-rollback-recovery.md) rather
than waiting for a human to notice.

**Compatibility check.** Before any package is staged, the Compatibility Checker evaluates, in
order: (1) signature and provenance verification against
[15 — Security Architecture](15-security-architecture.md); (2) for schema-bearing updates, whether
the manifest's `schema_range` covers the live schema version in
[29 — Database Schema](29-database-schema.md), and if not, whether a `migration_plan` is present
and itself reversible (§Recovery Mechanisms below requires every migration to declare an inverse);
(3) dependency compatibility — a Capability update is rejected if it would violate a dependent
Capability's declared contract version; (4) for models, hardware profile compatibility against the
local [Device Framework](20-device-framework.md) descriptor. Any failure here aborts before a
single byte is staged — there is no partial application of an incompatible update.

**Atomic apply, per track.**
- *System image*: write the full new image to the inactive slot, verify its content hash, flip
  the bootloader's active-slot pointer, and reboot into it with a bounded boot-attempt counter
  (§Architecture). This is the one track where "apply" and "activate" are separated by a reboot;
  every other track applies without one.
- *Capability*: install the new version into a fresh Trust Boundary alongside the running one,
  run its declared self-check (and, for higher-risk Capabilities, a shadow-traffic comparison
  gated by [23 — Multi-Model Orchestration](23-multi-model-orchestration.md)'s `shadow_evaluate`
  privacy check, same as model candidates), then swap the Capability Registry's active-version
  pointer — a single atomic write — and let in-flight invocations under the old version's
  capability tokens drain naturally.
- *Model*: the Model Router adds the new version to its candidate set once canary health clears,
  shifting new invocation routing weight toward it per the same staged percentages, while
  retaining the previous version as the fallback target ([23 — Multi-Model Orchestration](23-multi-model-orchestration.md)).

**Data migration under recovery-point protection.** Any migration accompanying an update runs as:
request a recovery point from [33 — Rollback & Recovery](33-rollback-recovery.md) → execute the
migration using the **expand/contract** pattern (add new schema shape, dual-write/backfill, only
*later* — in a separate, independently reversible update — remove the old shape) → validate row-
and graph-level invariants against [29 — Database Schema](29-database-schema.md) constraints →
commit, or invoke `rollback_to(recovery_point)` on any validation failure. Because the migration
never performs a destructive contract phase in the same update as the expand phase, a rollback of
the update is always a rollback to a schema shape that a broad prior population of the fleet is
still actively running.

## Interfaces / APIs

```
update_check(subject: UpdateSubject) -> Option<UpdateManifest>
update_fetch(manifest: UpdateManifest) -> Result<StagedPackage, FetchError>
compatibility_check(manifest: UpdateManifest) -> CompatibilityCheckResult
update_stage(package: StagedPackage) -> Result<(), StageError>
update_apply(subject: UpdateSubject, policy: RolloutPolicy) -> RolloutHandle
rollout_advance(handle: RolloutHandle) -> RolloutState
rollout_status(handle: RolloutHandle) -> RolloutState
update_rollback(subject: UpdateSubject, to_version: SemVer) -> RollbackReceipt   // delegates to 33
update_pin(subject: UpdateSubject, version: SemVer)   // opt device out of further rollout for now
```

Every one of these calls is itself an auditable, capability-secured operation
([02 §5](02-core-architecture.md#5-capability-security-as-the-unifying-security-model)): triggering
`update_apply` on anything but a user's own device, or on the system image without the
device-owner's consent capability, is rejected at the same kernel boundary described in
[03 — Kernel Architecture](03-kernel-architecture.md#capability-security-as-the-kernel-primitive).

## Pseudocode

```rust
/// Update Orchestrator's top-level driver, shared by all three tracks.
async fn apply_update(manifest: UpdateManifest) -> Result<(), UpdateError> {
    verify_signature(&manifest)?;                              // 15-security-architecture.md
    let compat = compatibility_check(&manifest);
    if !compat.schema_compatible || !compat.hardware_compatible
        || !compat.blocking_dependencies.is_empty() {
        return Err(UpdateError::Incompatible(compat));
    }

    let package = fetch_and_verify(&manifest.package_ref).await?; // 19-networking-stack.md, delta-aware
    stage(&manifest.subject, &package)?;                         // installed alongside current version

    // Recovery point precedes any state mutation, per 02 §4 invariant 2.
    let rp = recovery_point_create(
        Trigger::PreUpdate(manifest.subject.clone()),
    );                                                            // 33-rollback-recovery.md

    if let Some(plan) = &manifest.migration_plan {
        run_migration_expand_phase(plan).map_err(|e| {
            rollback_to(rp);                                      // 33-rollback-recovery.md
            UpdateError::MigrationFailed(e)
        })?;
    }

    for stage in &manifest.rollout_policy.stages {
        let cohort = select_cohort(*stage, &manifest.subject);
        activate_for_cohort(&manifest.subject, &package, cohort)?;
        sleep(manifest.rollout_policy.min_soak_time).await;

        let health = observability::cohort_health(&manifest.subject, cohort); // 34-*.md
        if !health.within(&manifest.rollout_policy.health_thresholds) {
            if manifest.rollout_policy.auto_rollback_on_breach {
                rollback_to(rp);                                  // full rollback, incl. migration
            }
            return Err(UpdateError::RolloutHealthBreach(health));
        }
    }

    commit_active_version(&manifest.subject, &manifest.to_version); // atomic pointer/slot flip
    emit_event(Event::UpdateCompleted(manifest.subject.clone()));   // 31-event-system.md
    Ok(())
}
```

## Security Considerations

Every package, at every stage of every track, must verify against a signed provenance chain
before it is even staged, per the capability-secured model in
[15 — Security Architecture](15-security-architecture.md); an unsigned or improperly-attested
package is rejected identically whether it is a kernel image, a Capability, or a model weight
file — there is exactly one verification path, mirroring the "exactly one security model"
requirement in [02 §5](02-core-architecture.md#5-capability-security-as-the-unifying-security-model).
The system image track additionally enforces **anti-rollback counters**: a signed monotonic
version counter prevents an attacker from reinstalling a deliberately-downgraded, vulnerable prior
image even though the A/B mechanism *can* boot old slots — downgrade is only permitted through the
explicit, audited `update_rollback` path, never through re-flashing an old signed image directly.
Telemetry consumed by the Rollout Cohort Controller is minimized and aggregated at the device tier,
never raw user content, consistent with [16 — Privacy Architecture](16-privacy-architecture.md).
Because updates are the highest-authority operation in the system, `update_apply` for the system
image track requires an explicit device-owner capability grant that cannot be delegated to an
Agent — no Intent, however phrased, can silently trigger a system image update; Capability and
model updates may be delegated to a lower-trust "keep my software current" policy the user sets
once, but that delegation is itself revocable exactly like any other capability grant
([03 §Algorithms](03-kernel-architecture.md#algorithms)). Offline and air-gapped deployments verify
the same signature chain from removable media rather than the network, so the security guarantee
does not depend on connectivity.

## Failure Modes

- **Power loss mid-flash.** The A/B scheme guarantees the active slot is never the write target,
  so a power loss while writing the inactive slot cannot affect the currently booted system;
  on next boot the bootloader simply retries staging.
- **New system image fails to boot.** The boot-attempt counter exhausts before a first-successful-
  boot health report; the bootloader reverts the active-slot pointer to the previous, still-intact
  slot with no user action.
- **Capability update crash-loops.** The supervisor tree ([03 §Recovery Mechanisms](03-kernel-architecture.md#recovery-mechanisms))
  restarts it under microreboot semantics up to a bounded retry count; exceeding that count
  quarantines the version and triggers `update_rollback` to the `prior_version` automatically.
- **Migration failure mid-transaction.** The expand-phase migration aborts atomically and
  `rollback_to(rp)` restores the pre-migration recovery point via
  [33 — Rollback & Recovery](33-rollback-recovery.md); the contract phase of any *prior* migration
  is untouched because contract phases only ever ship as their own, independently gated update.
- **Canary regression invisible to aggregate telemetry.** A regression affecting a narrow device
  or usage segment may not breach fleet-wide thresholds; mitigated by stratifying cohort health by
  device tier ([37 — Scalability Roadmap](37-scalability-roadmap.md)) rather than a single global
  average, and by keeping stage 1 small enough that a segment-specific regression is still visible
  in absolute counts.
- **Network partition during staged download.** Delta-patch downloads are resumable and
  content-hash-verified in chunks ([19 — Networking Stack](19-networking-stack.md)); a partial
  download is simply retried, never partially applied.
- **Model regression not caught by canary (accuracy/behavioral drift).** Mitigated by shadow
  evaluation — routing a sampled fraction of canary-cohort invocations through both old and new
  model versions and diffing structured outputs — before the Model Router shifts any live routing
  weight, per [23 — Multi-Model Orchestration](23-multi-model-orchestration.md).

## Recovery Mechanisms

Every apply path in §Pseudocode takes a recovery point immediately before mutating any
Storage-Engine-resident state, and delegates that half of reversal to
[33 — Rollback & Recovery](33-rollback-recovery.md) rather than implementing bespoke undo logic —
but the three tracks do not all reduce to one identical call. Reverting a Capability is a
registry-pointer flip back to `prior_version` plus draining the failed version's in-flight
invocations under revoked tokens, backed by a single `restore_to(recovery_point_id)` call (33) if
the failed version wrote any data worth discarding. Reverting a data migration replays its
declared inverse against the pre-migration recovery point via that same `restore_to` call, rather
than attempting to algebraically invert arbitrary writes. Reverting a **system image** is a
genuinely different, lighter mechanism that never calls `restore_to` at all: a bootloader-level
active-slot-pointer flip back to the previously-committed slot (`SystemImageSlot.committed`,
`boot_attempts_remaining`), fast and involving no data movement — because the four Storage Engine
stores are not slot-scoped in the first place; they are the same live data regardless of which
system image is currently booted, so there is no storage-level snapshot for a slot flip to revert.
When a system-image update shipped alongside a data migration, rolling it back is therefore two
coordinated steps under one user-facing "roll back this update" action, not one primitive that
does both: the slot-pointer flip (this document) *and* a `restore_to` call against the migration's
own pre-migration recovery point (33) — this is how "rolling back a bad update must also roll back
any data migration it performed" is actually satisfied, by composing two distinct reversal
mechanisms rather than by both tracks secretly being the same call.

## Performance Analysis

Delta patching keeps downloaded bytes proportional to the change, not the whole image or model
weight file, which matters most for the largest payloads (model weights can be gigabytes). Staging
the inactive slot or the shadow Capability/model version happens fully in the background while the
system remains responsive, so the user-visible cost of an update is the final pointer/slot flip —
sub-second for Capabilities and models, and one reboot (already budgeted under five seconds per
[01 §10](01-vision-and-philosophy.md#10-success-criteria)) for the system image. The A/B scheme
costs roughly double the system image's storage footprint at rest, which is small relative to
total device storage and is the direct price of the "never unbootable" guarantee. Canary rollout
velocity is a tunable trade-off against blast radius: an aggressive stage schedule (`[10%, 100%]`)
reaches full rollout faster at the cost of a larger population exposed before the first health
checkpoint; Hyperion's default schedule (`[1%, 10%, 50%, 100%]` with a minimum one-hour soak per
stage) favors blast-radius containment, consistent with the priority ordering in
[01 §5](01-vision-and-philosophy.md#5-universal-usability-highest-priority).

## Trade-offs

Maintaining independent versioning for the system image, every Capability, and every model
multiplies the compatibility-testing surface into a combinatorial matrix (a given Capability
version must remain compatible with a range of schema versions, model versions, and system image
versions simultaneously) — this is more ongoing engineering cost than a single monolithic "OS
version" number, and Hyperion accepts it because the alternative (one release train for
everything) directly violates the requirement that one bad Capability must never force a full OS
update. Staged/canary rollout delays full availability of a fix or feature for the whole fleet in
exchange for bounding the damage of a bad release to a small cohort; Hyperion always resolves this
tension toward containment, per the same priority ordering as above, though a user or
administrator may explicitly request an accelerated rollout for a specific device. Requiring every
migration to be expressible as an expand/contract pair constrains schema evolution — some
optimizations that would require an in-place, non-reversible transformation are disallowed or must
be split across two independently-gated updates — a constraint imposed directly by the reversibility
requirement in [33 — Rollback & Recovery](33-rollback-recovery.md) and accepted here as
non-negotiable rather than a case-by-case judgment call.

## Testing Strategy

Fault injection simulates power loss at every byte offset class during system-image flashing
(before, during, and after the content-hash verification step) to confirm the active slot is never
observably affected. A compatibility matrix — Capability version × Model version × Schema version ×
device tier — runs in continuous integration for the currently-supported version ranges, not only
the latest release. Canary regression detection is exercised by injecting synthetic health-metric
degradation into a simulated cohort and asserting the Rollout Cohort Controller halts and, where
`auto_rollback_on_breach` is set, triggers rollback within a bounded time budget. Every migration's
declared inverse is tested by round-tripping a representative dataset through expand → contract →
inverse and asserting bit-for-bit equivalence with the pre-migration state. Rollback drills — apply,
deliberately fail a downstream health check, and verify the device returns to the exact prior
content-hash state — are a required, non-optional stage of the release pipeline for every track,
not an occasional exercise, because the promise this document makes ("never unbootable, never an
inconsistent Knowledge Graph") is only true if it is continuously verified rather than assumed.

---
*Next: [33 — Rollback & Recovery](33-rollback-recovery.md).*
