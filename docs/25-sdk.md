# SDK

## Purpose

This document specifies the **Capability SDK** — the developer-facing toolchain used to build,
test, and publish a [Capability](02-core-architecture.md#capability), Hyperion's replacement for
the application. It covers project scaffolding, the declarative contract language a developer
writes against, a local emulator/sandbox for testing a Capability before it ever touches a real
user's data, a golden-input/golden-output regression harness, and the marketplace submission flow.
It explicitly does **not** own the wire format of the manifest a Capability compiles down to, the
sandboxing model that runs it, or the registry that indexes it — those belong to
[24 — Plugin Framework](24-plugin-framework.md). It does not own the runtime API surface a
Capability calls into during execution — that belongs to [26 — APIs](26-apis.md). It does not own
how legacy, non-Capability software is hosted — that belongs to
[27 — Compatibility Layer](27-compatibility-layer.md). The SDK is the tool that *produces* what
those three documents consume, run, and secure.

## Motivation

[01 §4](01-vision-and-philosophy.md#4-primary-design-philosophy) establishes that the unit of
installable software in Hyperion is a Capability, not an application bundle, and that the OS —
never the developer, never the user — chooses which **implementation** of a Capability satisfies
a given [Intent](02-core-architecture.md#intent) at a given moment. This single decision reshapes
what "developer tooling" must mean. A developer building a traditional application controls its
entire runtime environment; a developer building a Capability controls only its *contract* — the
[Model Router](23-multi-model-orchestration.md) may run their implementation on a local NPU today,
route to a competitor's cloud implementation tomorrow under a different privacy policy, or run
both side by side to score them. If a Capability's declared behavior can drift from its actual
behavior across that substitution, the [Model Router](23-multi-model-orchestration.md)'s dynamic
selection stops being safe, and every downstream [Agent](02-core-architecture.md#agent) or
[Workspace](02-core-architecture.md#workspace) built on top of it inherits the surprise. The SDK
exists to make **swappability without behavior surprise** a property the tooling enforces, not a
property developers are trusted to remember. [01 §2's](01-vision-and-philosophy.md#2-the-golden-rule)
golden rule applies here to a different human than usual: the developer is the person whose goal
("publish a correct, safely-scoped, swappable Capability quickly") the SDK must make easier.

## Architecture

The SDK is a linear pipeline from a scaffolded project to a signed, registry-published Capability,
with two mandatory gates — the local emulator and the marketplace review — that a Capability
cannot bypass regardless of how it was authored.

```
┌──────────────────────────────────────────────────────────────────────────┐
│ DEVELOPER WORKSTATION                                                     │
│                                                                            │
│  hyperion scaffold ──▶ Capability Project                                 │
│                          ├─ contract.cap.ts     (§Data Structures)        │
│                          ├─ impl/<name>.ts      (1..N Implementations)    │
│                          ├─ tests/golden/*.json (golden I/O cases)        │
│                          └─ hyperion.lock       (resolved dependency set) │
│                                   │                                        │
│                                   ▼                                       │
│                          hyperion emulate  (Trust Boundary, depth 0/1)    │
│                          ┌──────────────────────────────────────────┐    │
│                          │ Mock Context Bundle   (synthetic, 06/07) │    │
│                          │ Mock Knowledge Graph  (seeded, 09)       │    │
│                          │ Mock Model Router     (deterministic,23) │    │
│                          └──────────────────────────────────────────┘    │
│                                   │                                        │
│                                   ▼                                       │
│                          hyperion test                                    │
│                          ├─ Contract conformance (I/O shape vs. spec)     │
│                          ├─ Golden regression    (semantic diff)         │
│                          └─ Cross-implementation equivalence              │
│                                   │                                        │
│                                   ▼                                       │
│                          hyperion publish                                 │
└───────────────────────────────────┼───────────────────────────────────────┘
                                     ▼
                      MARKETPLACE SUBMISSION SERVICE
                      ├─ Static permission analysis (declared vs. used)
                      ├─ Review Gate — sensitive permissions ⇒ human review
                      │   (15-security-architecture.md)
                      └─ Signing ──▶ manifest emitted in 24's format
                                     │
                                     ▼
                         24 — Plugin Framework: Capability Registry
```

**Scaffold.** `hyperion scaffold <template>` generates a project from a template (translation,
summarizer, scheduler-integration, generic) with a starter contract, one starter implementation,
and a starter golden case, so a first Capability compiles and passes its own tests before a line
of business logic is written — the SDK equivalent of [01 §5's](01-vision-and-philosophy.md#5-universal-usability-highest-priority)
"never wonder what to do next," aimed at the developer.

**Emulate.** The local emulator runs the Capability inside the same Trust Boundary depth it would
run at in production (per [03 — Kernel Architecture](03-kernel-architecture.md#sandboxing-as-one-spectrum)),
against synthetic stand-ins for the three systems a Capability normally depends on but a developer
should never need live, production access to just to iterate: a Context Bundle, a Knowledge Graph
slice, and the Model Router. This is what lets a Capability be fully exercised — including its
declared side effects and permission requests — before it ever sees a real user's data.

**Test.** The harness runs three kinds of check, detailed in §Algorithms: contract conformance,
golden regression, and — the check unique to Hyperion's dynamic-implementation model —
cross-implementation equivalence, which fails the build if two implementations of the same
contract disagree on the same golden input beyond a declared tolerance.

**Publish.** `hyperion publish` performs static permission analysis, then hands the package to the
review gate defined jointly with [15 — Security Architecture](15-security-architecture.md), and on
approval, signs it and emits the manifest consumed by [24 — Plugin Framework](24-plugin-framework.md).

## Data Structures

```typescript
// The declarative contract: typed inputs/outputs/side-effects/permissions.
// Compiles to the manifest format owned by 24-plugin-framework.md — the SDK
// never redefines that format, only produces valid instances of it.
interface Contract {
  id: string;                       // e.g. "legal.translate"
  version: SemVer;
  summary: string;                  // shown to users via 18-explainability-and-trust.md
  inputs: Record<string, SemanticType>;
  outputs: Record<string, SemanticType>;
  sideEffects: SideEffect[];        // e.g. KnowledgeGraphWrite, NotificationSend
  permissionsRequested: Permission[];
  trustLevel: "sandboxed" | "standard" | "elevated";
}

// One or more Implementations satisfy a single Contract; the OS — never the
// developer — selects among them at invocation time (02 — Capability).
interface Implementation {
  contractId: string;
  name: string;                     // e.g. "local-nmt-v3", "cloud-frontier-v1"
  runtime: LocalModel | CloudAPI | NativeBinary | ComposedCapability;
  resourceProfile: ResourceProfile; // declared shape; defined and resolved by 04-scheduler.md
  latencyClass: "interactive" | "batch";
  requiresConsent: boolean;         // true if it crosses a Trust Boundary (16)
}

// A golden regression case: one recorded (input, expected-output) pair plus
// the tolerance the semantic diff (§Algorithms) is allowed to apply.
interface GoldenCase {
  caseId: string;
  contextBundle: MockContextBundle;   // synthetic Context Bundle, 06/07
  input: Record<string, unknown>;
  expectedOutput: Record<string, unknown>;
  tolerance: { structural: "exact"; content: number };  // 0.0–1.0 embedding distance
}

// What the local emulator seeds itself with — never live user data.
interface EmulatorProfile {
  knowledgeGraphSeed: SemanticObjectFixture[];  // 09-knowledge-graph.md shapes
  contextBundleFixtures: MockContextBundle[];
  modelRouterStub: "deterministic" | "recorded-replay";
}

interface PublishSubmission {
  packageHash: Sha256;
  contract: Contract;
  declaredPermissions: Permission[];
  staticallyObservedPermissions: Permission[];  // from analysis, §Algorithms
  reviewStatus: "auto-approved" | "pending-human-review" | "rejected";
}
```

## Algorithms

**Contract compilation.** The CDL (Capability Definition Language) source in `contract.cap.ts` is
type-checked, then lowered to the manifest schema owned by [24](24-plugin-framework.md); the SDK's
compiler is intentionally the *only* code path permitted to emit a valid manifest, so the manifest
format can evolve without every developer hand-authoring it.

**Structural implementation compatibility.** Because multiple, independently authored
Implementations must satisfy one Contract, compatibility is checked structurally rather than
nominally: an Implementation's declared input types must be contravariant with (equal to or wider
than) the Contract's, and its output types covariant with (equal to or narrower than) the
Contract's. This lets a `cloud-frontier-v1` implementation accept a strict superset of input
formats and still satisfy a Contract that only promises a subset, without inheritance or shared
base classes between unrelated developers' code.

**Golden regression (semantic diff).** Because implementations are frequently generative models,
byte-exact snapshot comparison is unusable — the correct translation of a contract clause is not a
unique string. The harness applies a two-layer diff: (1) **structural**, exact-match on output
*shape* (the right [Semantic Object](02-core-architecture.md#semantic-object) types and relations
are present — a translation must still be a `Document`, still carry a `translationOf` edge into
the [Knowledge Graph](09-knowledge-graph.md)); (2) **content**, an embedding-distance comparison
against `expectedOutput`, failing only if distance exceeds the case's declared `tolerance.content`.
This is what lets a Capability be swapped between implementations without behavior surprise: the
harness proves the shape invariant always holds and the content stays within a developer-chosen
similarity band, rather than proving byte-identical output, which dynamic model selection makes
impossible to guarantee or even desirable.

**Cross-implementation equivalence.** The same golden suite is run against every Implementation
declared for a Contract, not just the one under active development; a failure here — one
implementation passing a golden case and another failing it beyond tolerance — blocks publish,
because it is precisely the condition [23 — Multi-Model Orchestration](23-multi-model-orchestration.md)
depends on the SDK to have ruled out before it will treat two implementations as interchangeable.

**Static permission analysis.** The compiler scans an Implementation's source for calls into the
[Capability Invocation API](26-apis.md) and any declared side effect emitters, and diffs the
observed permission surface against `permissionsRequested` in the Contract. Any observed use
exceeding the declared set fails the build before submission; any declared-but-unused permission is
flagged as a warning (unused sensitive scopes are themselves a review-gate risk, per
[15 — Security Architecture](15-security-architecture.md)).

## Interfaces / APIs

```
hyperion scaffold <template> [--name <id>]        # generate a Capability project
hyperion emulate [--profile <EmulatorProfile>]     # run against mock Context/KG/Router
hyperion test [--golden] [--equivalence]           # run the test harness (§Algorithms)
hyperion golden record <caseId>                    # capture a new golden case from a live run
hyperion golden rebaseline <caseId>                # human-approved update to an existing case
hyperion lint --permissions                        # static permission analysis only
hyperion publish [--channel beta|stable]           # submit to the marketplace (§Architecture)
hyperion status <submissionId>                     # poll review-gate status
```

```typescript
// SDK library surface used inside contract.cap.ts and impl/*.ts
function defineCapability(contract: Contract): void;
function defineImplementation(impl: Implementation): void;
function mockContextBundle(fixture: Partial<MockContextBundle>): MockContextBundle;
function mockKnowledgeGraph(seed: SemanticObjectFixture[]): MockKnowledgeGraphHandle;
function runGolden(cases: GoldenCase[]): GoldenReport;
function publish(pkg: PublishSubmission): Promise<PublishReceipt>;
```

## Pseudocode

```typescript
// contract.cap.ts — the worked example from the source brief.
defineCapability({
  id: "legal.translate",
  version: "1.0.0",
  summary: "Translate legal documents while preserving legal terminology and formatting.",
  inputs: { document: oneOf([Sem.PDF, Sem.DocxDocument, Sem.Image]), targetLanguage: Sem.LanguageTag },
  outputs: { translated: Sem.Document, confidence: Sem.Confidence },
  sideEffects: [SideEffect.KnowledgeGraphWrite({ relation: "translationOf" })],
  permissionsRequested: [
    Permission.Read("semantic-object:document"),
    Permission.Write("semantic-object:document", { scope: "new-object-only" }),
    Permission.Network("model-inference", { classification: "may-require-cloud" }),
  ],
  trustLevel: "standard",
});

defineImplementation({
  contractId: "legal.translate", name: "local-nmt-v3",
  runtime: LocalModel("nmt-legal-7b"), resourceProfile: { npu: "preferred", memory: "2GiB" },
  latencyClass: "interactive", requiresConsent: false,
});
defineImplementation({
  contractId: "legal.translate", name: "cloud-frontier-v1",
  runtime: CloudAPI("partner.translate.v2"), resourceProfile: { network: "required" },
  latencyClass: "batch", requiresConsent: true,   // crosses Trust Boundary, see 16
});

// Test harness runner (simplified) — this is what `hyperion test --golden --equivalence` executes.
function runHarness(contract: Contract, impls: Implementation[], goldens: GoldenCase[]): Report {
  const report: Report = { pass: [], fail: [] };
  for (const impl of impls) {
    const sandbox = emulator.spawn(impl, { depth: trustDepthFor(contract.trustLevel) });
    for (const gc of goldens) {
      const actual = sandbox.invoke(gc.input, gc.contextBundle);

      // Layer 1: structural — must hold exactly, regardless of tolerance.
      if (!shapeMatches(actual, gc.expectedOutput)) {
        report.fail.push({ impl: impl.name, case: gc.caseId, reason: "structural-mismatch" });
        continue;
      }
      // Layer 2: content — embedding distance within the declared band.
      const dist = embeddingDistance(actual.content, gc.expectedOutput.content);
      if (dist > gc.tolerance.content) {
        report.fail.push({ impl: impl.name, case: gc.caseId, reason: `content-drift:${dist}` });
      } else {
        report.pass.push({ impl: impl.name, case: gc.caseId });
      }
    }
  }
  // Cross-implementation equivalence: same case must not diverge in pass/fail across impls.
  for (const gc of goldens) {
    const verdicts = impls.map(i => report.pass.some(p => p.impl === i.name && p.case === gc.caseId));
    if (new Set(verdicts).size > 1) {
      report.fail.push({ impl: "cross-implementation", case: gc.caseId, reason: "equivalence-violation" });
    }
  }
  return report;
}
```

## Security Considerations

The emulator is required to run at the **same minimum Trust Boundary depth** the Capability would
receive in production (per [03 — Kernel Architecture](03-kernel-architecture.md#sandboxing-as-one-spectrum));
testing a Capability at a laxer depth than it ships with would hide exactly the privilege issues
the harness exists to catch. Sensitive permission requests — cross-device network access, broad
[Knowledge Graph](09-knowledge-graph.md) write scope, [Memory](08-memory-engine.md) read access —
force the human review gate defined jointly with [15 — Security Architecture](15-security-architecture.md)
regardless of how well-tested a Capability is; passing golden tests is a correctness signal, not a
trust signal, and the SDK never conflates the two. Published packages are signed at the end of the
pipeline; the [Capability Registry](24-plugin-framework.md) refuses to install an unsigned or
tampered package. Dependency Capabilities (a Capability composed from others, per
[02 — Capability](02-core-architecture.md#capability)) are pinned by content hash in
`hyperion.lock`, closing the supply-chain substitution attack where a dependency is silently
swapped between test and publish. Contract and fixture files are scanned for embedded secrets
before emulation; the emulator refuses to boot a profile containing anything that looks like a
live credential, forcing developers to keep secrets out of source entirely.

## Failure Modes

- **Golden nondeterminism.** Generative implementations can produce marginally different valid
  output on every run, producing false regression failures if tolerance is set too tight.
- **Emulator/production schema drift.** The mock Knowledge Graph's fixture shapes fall out of sync
  with the real schema as [09 — Knowledge Graph](09-knowledge-graph.md) evolves, letting a
  Capability pass locally and fail in production.
- **Static analysis blind spots.** Dynamically constructed permission requests (a string built at
  runtime rather than a literal in source) can evade the compiler's permission scan.
- **Review-gate backlog.** A queue of pending human reviews can block an urgent security patch to
  an already-shipped Capability that does not itself change permission scope.

## Recovery Mechanisms

Tolerance-banded diffing keeps most nondeterminism from becoming a false failure; when it does, the
`hyperion golden rebaseline` workflow requires an explicit, logged developer sign-off to update a
golden case, so re-baselining is itself auditable and cannot silently paper over a real regression.
The emulator's schema is version-pinned to a specific [09 — Knowledge Graph](09-knowledge-graph.md)
schema revision, and `hyperion emulator doctor` diffs the pinned schema against the live one on
every CI run, failing loudly rather than letting drift accumulate silently. Runtime permission
escalation attempts that evade static analysis are still blocked at invocation time by the same
capability-token enforcement described in [03 — Kernel Architecture](03-kernel-architecture.md) —
the static scan is a fast-feedback convenience, not the enforcement boundary, so a missed case fails
safe rather than fails open. Review-gate backlog is mitigated with an expedited lane: a submission
whose statically-observed permission surface is unchanged from the previously approved version
skips back to the fast lane; only submissions that *expand* permission scope wait on full review.

## Performance Analysis

Scaffold and emulator cold-start are budgeted at low seconds, not minutes, so the local
inner-development loop stays interactive; the golden suite is budgeted to complete in well under a
minute for a typical Capability so that `hyperion test --equivalence` can run on every implementation
added without becoming a reason to skip it. Contract type-checking is incremental — only the
Contract and Implementations touched since the last build are re-checked — to keep the CLI
responsive as a Capability's implementation set grows. These budgets are enforced the same way
[36 — Performance Benchmarks](36-performance-benchmarks.md) enforces system-level targets: as a
gate the SDK's own CI treats as a regression, not an aspiration.

## Trade-offs

Structural (duck-typed) contract compatibility, rather than nominal inheritance between
Implementations, was chosen so unrelated developers can each ship an implementation of the same
Contract without coordinating a shared base type; the cost is less precise compiler diagnostics
when a mismatch occurs, which the SDK offsets with targeted structural-diff error messages rather
than a generic type error. Semantic-diff golden testing, rather than exact-match snapshotting, is
the only workable choice given that implementations are frequently generative and non-deterministic
— but a similarity tolerance that is too loose risks masking a genuine regression as acceptable
drift. The layered diff (exact-match structure, tolerance-banded content) is the mitigation: it
never allows the *shape* of an output — its Semantic Object type, its graph relations, its declared
side effects — to drift at all, confining tolerance to content quality where some variance is
inherent and expected.

## Testing Strategy

Every Capability built with the SDK is tested three ways before publish: contract conformance
(does output match declared shape), golden regression (does content stay within the declared
tolerance of a known-good answer), and cross-implementation equivalence (do all of a Contract's
implementations agree closely enough to be treated as interchangeable by
[23 — Multi-Model Orchestration](23-multi-model-orchestration.md)). The SDK tool itself is tested
independently: a scaffold-and-build smoke test on every template runs in CI on every SDK release; a
canary suite of in-house reference Capabilities is re-run against every new SDK version to catch
tooling regressions before they reach third-party developers; and a CDL compatibility matrix
verifies that Contracts authored against older SDK versions still compile, so a developer is never
silently broken by a platform upgrade.

---
*Next: [26 — APIs](26-apis.md).*
