# Plugin Framework

## Purpose

This document specifies the **Plugin Framework**, the L2 Platform Services component (per
[02 — Core Architecture §1](02-core-architecture.md#1-layered-system-view)) responsible for
packaging, installing, sandboxing, and registering everything a third party can contribute to
Hyperion. Per the founding brief — "applications are replaced by capabilities; the operating
system decides which software, models, APIs, or services fulfill each capability" — a Plugin is
never an application in the traditional sense. It is a signed package that contributes one or more
of: [Capabilities](02-core-architecture.md#capability), [Agents](11-agent-runtime.md), models
(consumed by [23 — Multi-Model Orchestration](23-multi-model-orchestration.md) and executed by
[22 — Local AI Runtime](22-local-ai-runtime.md)), hardware support (drivers, per
[20 — Device Framework](20-device-framework.md)), knowledge providers (feeding
[09 — Knowledge Graph](09-knowledge-graph.md)), UI components (consumed by
[13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md)), execution engines, automation workflows, or
memory providers (feeding [08 — Memory Engine](08-memory-engine.md)). This document defines the
manifest format such contributions are declared in, the install-time sandboxing model, the
registry the [Intent Engine](05-intent-engine.md) and [Model Router](23-multi-model-orchestration.md)
query, and how the OS resolves two Plugins that both claim to satisfy the same Capability.

## Motivation

[02 — Capability](02-core-architecture.md#capability) defines a Capability as declaring "a
semantic contract … zero or more implementations … a trust level and resource profile," and states
plainly that the OS, not the user or developer, chooses which implementation runs. That sentence
only holds if there is a uniform, machine-readable artifact describing every implementation before
it runs, and a uniform enforcement point stopping it from doing anything beyond what it declared.
The Plugin Framework is that artifact's producer-side contract and its enforcement point: it is
what the developer tooling in [25 — SDK](25-sdk.md) *produces*, and what
[03 — Kernel Architecture](03-kernel-architecture.md)'s Trust Boundary machinery *consumes*. Without
it, "the OS decides" degrades into "whatever code happens to be running decides," which violates
[02 §4](02-core-architecture.md#4-design-invariants)'s "no silent authority" invariant on day one of
any third-party ecosystem.

## Architecture

```
   Developer, using 25-sdk.md tooling
                │  produces
                ▼
      ┌───────────────────────┐
      │     Plugin Package      │   signed manifest + contribution artifacts
      └───────────┬────────────┘
                  ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│                    PLUGIN FRAMEWORK  (L2, this document)                       │
│                                                                                 │
│  ┌─────────────────┐   ┌───────────────────────┐   ┌─────────────────────┐   │
│  │ Manifest           │──▶│ Consent / Permission   │──▶│ Trust Boundary       │   │
│  │ Validator           │   │ Diff UI                 │   │ Admission            │   │
│  │ schema · signature  │   │ (01 §9, 18) — only new   │   │ sandbox_create(depth) │   │
│  │ · over-request check│   │ or changed grants shown  │   │ per 03's spectrum     │   │
│  └─────────────────┘   └───────────────────────┘   └──────────┬───────────┘   │
│                                                                    ▼               │
│                                            ┌──────────────────────────────────┐   │
│                                            │      Capability Registry           │   │
│                                            │ indexed by capability_id AND by    │   │
│                                            │ semantic embedding of contract      │   │
│                                            │ (via 09 — Knowledge Graph)          │   │
│                                            └────────────────┬─────────────────┘   │
└──────────────────────────────────────────────────────────────┼───────────────────┘
              ┌───────────────────────────────────────────────┼──────────────────────┐
              ▼                                                 ▼                        ▼
   ┌───────────────────────┐                        ┌───────────────────────┐   ┌───────────────────┐
   │ Intent Engine (05)       │                        │ Model Router (23)       │   │ Update System (32)  │
   │ "what capability do I     │                        │ "which implementation    │   │ versioned upgrades,  │
   │ need for this Intent?"    │                        │ of it should run now?"    │   │ staged rollout        │
   └───────────────────────┘                        └───────────────────────┘   └───────────────────┘
```

### Plugin contributions

A single Plugin package may declare any combination of eight contribution kinds named in the
brief. Each kind maps onto an existing subsystem's extension point rather than inventing a parallel
one: Capabilities register into this document's registry; Agents register into
[11 — Agent Runtime](11-agent-runtime.md)'s roster; Models register as
`ImplementationDescriptor`s consumed by [23 — Multi-Model Orchestration](23-multi-model-orchestration.md);
hardware support registers drivers into [20 — Device Framework](20-device-framework.md); knowledge
providers register ingestion sources into [09 — Knowledge Graph](09-knowledge-graph.md); UI
components register renderable widgets into [13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md);
execution engines register runtimes usable by Capability implementations; automation workflows
register composite, multi-step Capabilities; memory providers register storage backends into
[08 — Memory Engine](08-memory-engine.md). The Plugin Framework's job is uniform across all eight:
validate, sandbox, register, and make discoverable — the contribution-specific schema is a payload
this document's envelope carries, not a reason to fork the framework eight ways.

### Installation and sandboxing flow

Installation is: (1) validate the manifest — schema conformance, publisher signature, and a static
check that requested permissions are a subset of what the declared contract's side effects actually
require, rejecting over-broad requests outright; (2) compute a permission *diff* against anything
already granted (relevant on update) and show the user only what is new, per
[01 §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable)'s auditability requirement
and explained per [18 — Explainability & Trust](18-explainability-and-trust.md); (3) on consent,
admit the Plugin into a fresh [Trust Boundary](02-core-architecture.md#trust-boundary) using
[03 — Kernel Architecture](03-kernel-architecture.md#algorithms)'s existing
`sandbox_create(depth, profile)` admission algorithm — depth chosen from the Plugin's declared
trust level, possibly promoted deeper by device policy in
[15 — Security Architecture](15-security-architecture.md); (4) mint exactly the capability tokens
the user approved, no more; (5) register every contribution into the appropriate subsystem index.
Every Plugin, without exception, runs inside a Trust Boundary — there is no "trusted first-party"
exemption, because a uniform enforcement point is what makes the security model in
[02 §5](02-core-architecture.md#5-capability-security-as-the-unifying-security-model) genuinely
singular rather than singular-with-carve-outs.

### Registry and discovery

The Capability Registry is indexed two ways: exactly, by `capability_id`, for direct lookups (what
the [Model Router](23-multi-model-orchestration.md) uses once it knows which Capability an Intent
needs); and semantically, by an embedding of the contract's natural-language description, inserted
into [09 — Knowledge Graph](09-knowledge-graph.md), for the [Intent Engine](05-intent-engine.md)'s
harder problem of figuring out *which* Capability, if any, a freshly parsed natural-language Intent
actually needs. A user asking to "put this into Portuguese" should resolve to
`document.translate.legal` (§Worked Example) via semantic proximity even though the Intent never
says "translate" or "legal."

### Versioning and conflict resolution

Two Plugins offering the same `capability_id` is the *normal*, encouraged case, not an error — it
is exactly what gives the [Model Router](23-multi-model-orchestration.md) multiple implementations
to choose between, per [02 — Capability](02-core-architecture.md#capability)'s "zero or more
implementations." The Plugin Framework only needs to adjudicate when two registrations are not
compatible: it runs a structural compatibility check on the declared input/output/side-effect
schema. If it matches, the second Plugin is registered as an *additional* `ImplementationDescriptor`
under the existing Capability — a routing question for 23, not an installation-time question here.
If the schemas diverge (different required inputs, different side effects, or an incompatible
privacy tier), the Plugin Framework registers it under a distinct, versioned `capability_id`
variant and leaves disambiguation to the Intent Engine's semantic matching and, where genuinely
ambiguous, to the user. Version upgrades of a single Plugin follow semantic versioning: a
compatible minor/patch upgrade replaces the descriptor in place; a breaking major upgrade keeps the
previous version's descriptor pinned and reachable until dependents migrate, coordinated with
[32 — Update System](32-update-system.md)'s staged rollout and reversible via
[33 — Rollback & Recovery](33-rollback-recovery.md).

## Data Structures

```rust
struct PluginManifest {
    plugin_id: PluginId,
    version: SemVer,
    publisher: PublisherIdentity,     // verified against a signing key, per 15
    signature: Signature,
    sdk_version: SemVer,              // must satisfy this document's compatibility floor
    contributions: Vec<Contribution>,
    requested_permissions: Vec<CapabilityGrantRequest>,
    min_trust_depth: TrustDepth,      // input to 03's sandbox_create
    resource_profile: ResourceProfile, // declared shape, defined in 04-scheduler.md
}

enum Contribution {
    Capability(CapabilityManifest),
    Agent(AgentManifest),                  // -> 11
    Model(ModelManifest),                  // -> 22, 23
    HardwareSupport(DriverManifest),       // -> 20
    KnowledgeProvider(KnowledgeProviderManifest), // -> 09
    UiComponent(UiComponentManifest),      // -> 13
    ExecutionEngine(ExecutionEngineManifest),
    AutomationWorkflow(WorkflowManifest),
    MemoryProvider(MemoryProviderManifest),// -> 08
}

struct CapabilityManifest {
    capability_id: CapabilityId,
    contract: SemanticContract {
        inputs: Vec<TypeDescriptor>,
        outputs: Vec<TypeDescriptor>,
        side_effects: Vec<SideEffect>,     // e.g. CreatesSemanticObject, NetworkEgress, None
    },
    implementation_kind: ImplKind,          // LocalSmallModel | LocalLargeModel | CloudAPI | NativeBinary
    privacy_tier: PrivacyTier,               // 16
    consequence_tier: ConsequenceTier,       // declared in 23; consumed there and by 15
    quality_hooks: BenchmarkHarnessRef,      // how 23's benchmark table evaluates this impl
    version: SemVer,
}

struct CapabilityGrantRequest {
    operation: Operation,        // e.g. Read(SemanticObjectClass), NetworkEgress(Domain), Write(...)
    scope: Scope,
    justification: String,       // shown verbatim in the consent UI, per 18
}

struct RegistryEntry {
    capability_id: CapabilityId,
    implementations: Vec<ImplementationDescriptor>,  // 23's type — this doc populates it
    owning_plugins: Vec<PluginId>,
    install_state: InstallState,   // Pending | Active | Quarantined | Revoked
}
```

## Algorithms

**1. Manifest validation.** Schema-check the manifest against the current SDK schema version;
verify `signature` against `publisher`'s registered key ([15 — Security Architecture](15-security-architecture.md));
statically cross-check `requested_permissions` against the `side_effects` declared in each
`CapabilityManifest` the package contributes — a Capability declaring `side_effects: [None]` that
requests `NetworkEgress` is rejected before it ever reaches a user, since an over-broad request is a
red flag regardless of intent.

**2. Consent diff.** On first install, present every requested grant. On update, present only
grants absent from the currently-held set; unchanged grants are carried forward without
re-prompting, so a Plugin cannot exhaust user attention into rubber-stamping by re-asking for
things already approved — this is what keeps consent meaningful rather than fatiguing, per
[01 §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable).

**3. Trust Boundary admission.** Delegates directly to
[03 — Kernel Architecture](03-kernel-architecture.md#algorithms)'s admission algorithm:
`sandbox_create(depth, profile)`, where `depth` is the greater of the Plugin's declared
`min_trust_depth` and any deeper minimum [15 — Security Architecture](15-security-architecture.md)
imposes for this device/user policy. Capability tokens are minted only for the exact, user-approved
`CapabilityGrantRequest`s — never a superset "just in case."

**4. Discovery indexing.** For each `CapabilityManifest`, insert an embedding of its contract
description into [09 — Knowledge Graph](09-knowledge-graph.md)'s index, keyed to the
`RegistryEntry`, so nearest-neighbor semantic lookups from the [Intent Engine](05-intent-engine.md)
resolve to it even without an exact name match.

**5. Conflict resolution.** For an incoming `CapabilityManifest` whose `capability_id` already has
a `RegistryEntry`: run a structural diff on `contract`. Compatible (same input/output/side-effect
shape, any privacy/consequence tier) → append as a new `ImplementationDescriptor` under the
existing entry, handing [23 — Multi-Model Orchestration](23-multi-model-orchestration.md) a genuine
choice. Incompatible → mint a new, distinctly versioned `capability_id`
(`document.translate.legal.v2` style) and let semantic discovery and, if truly ambiguous, direct
user choice disambiguate at Intent-resolution time rather than at install time.

**6. Uninstall / revocation.** Revoking a Plugin calls
[03 — Kernel Architecture](03-kernel-architecture.md#algorithms)'s `cap_revoke` across the Plugin's
entire capability set — one graph walk invalidates everything it was delegated — then removes its
`RegistryEntry` contributions. In-flight invocations against a revoked implementation surface
`Fault::Revoked`, which [23 — Multi-Model Orchestration](23-multi-model-orchestration.md)'s fallback
chain absorbs transparently.

## Interfaces / APIs

```
plugin_install(pkg: PluginPackage) -> InstallResult
plugin_request_grant(plugin_id: PluginId, grants: Vec<CapabilityGrantRequest>) -> ConsentDecision
plugin_uninstall(plugin_id: PluginId) -> RevocationReceipt
plugin_update(plugin_id: PluginId, pkg: PluginPackage) -> UpdateResult        // ties to 32
registry_query(query: CapabilityQuery) -> Vec<ImplementationDescriptor>       // used by 05, 23
registry_register_implementation(plugin_id: PluginId, m: CapabilityManifest) -> ImplementationDescriptor
registry_quarantine(plugin_id: PluginId, reason: QuarantineReason) -> ()      // ties to 17
```

`CapabilityQuery` accepts either an exact `capability_id` (the Model Router's usual path) or a
semantic embedding plus similarity threshold (the Intent Engine's usual path), so both consumers
named in the brief share one entry point.

## Pseudocode

```rust
fn plugin_install(pkg: PluginPackage, registry: &mut CapabilityRegistry) -> InstallResult {
    let manifest = validate_manifest(&pkg)?;              // schema, signature, over-request check
    for req in &manifest.requested_permissions {
        if !contract_requires(&manifest, req) {
            return InstallResult::Rejected(RejectReason::OverBroadPermission(req.clone()));
        }
    }

    let existing_grants = grants_for(manifest.plugin_id);
    let new_grants: Vec<_> = manifest.requested_permissions.iter()
        .filter(|g| !existing_grants.contains(g))
        .cloned().collect();
    let decision = present_consent_ui(&manifest.plugin_id, &new_grants);   // -> 01 §9, 18
    if decision != ConsentDecision::Approved {
        return InstallResult::DeclinedByUser;
    }

    let depth = max(manifest.min_trust_depth, policy_minimum_depth(&manifest)); // -> 15
    let boundary = sandbox_create(depth, manifest.resource_profile.clone());    // -> 03
    let tokens = mint_tokens(&boundary, &manifest.requested_permissions);       // exact set, no superset

    for contribution in &manifest.contributions {
        match contribution {
            Contribution::Capability(cm) => {
                let desc = build_implementation_descriptor(cm, manifest.plugin_id, tokens.clone());
                match registry.entry(cm.capability_id.clone()) {
                    Entry::Vacant(v) => { v.insert(RegistryEntry::new(desc)); }
                    Entry::Occupied(mut e) => {
                        if structurally_compatible(&e.get().contract(), &cm.contract) {
                            e.get_mut().implementations.push(desc);   // -> feeds 23
                        } else {
                            let versioned_id = version_variant(&cm.capability_id);
                            registry.insert(versioned_id, RegistryEntry::new(desc));
                        }
                    }
                }
                index_semantic_embedding(cm, boundary);               // -> 09, feeds 05
            }
            other => register_into_owning_subsystem(other, manifest.plugin_id, boundary.clone()),
        }
    }
    InstallResult::Installed { boundary }
}
```

## Worked Example: "Translate Legal Documents"

The brief's own example — inputs PDF/Word/Image, outputs a translated semantic document — worked
through the manifest format:

```yaml
plugin_id: com.example.legal-translate
version: 1.2.0
publisher: { name: "Example Legal AI", key_fingerprint: "..." }
sdk_version: "1.0"
contributions:
  - capability:
      capability_id: document.translate.legal
      contract:
        inputs:
          - { type: SemanticObject, of_class: Document, formats: [pdf, docx, image] }
          - { type: Parameter, name: target_language, schema: BCP47 }
        outputs:
          - { type: SemanticObject, of_class: Document,
              preserves: [layout, clause_anchors, formatting] }
        side_effects: [CreatesSemanticObject]
      implementation_kind: LocalSmallModel
      privacy_tier: FullyLocal         # legal documents default to local-only, per 16
      consequence_tier: Sensitive      # legal consequence tier, per 15/23 — a separate axis
                                        # from privacy_tier above (see 23 §Ensemble/verification)
      quality_hooks: { harness: legal-translation-eval-v3 }
      version: 1.2.0
requested_permissions:
  - operation: Read(SemanticObjectClass::Document)
    scope: { workspace_local: true }
    justification: "Reads the source document you selected to translate it."
  - operation: Write(SemanticObjectClass::Document)
    scope: { workspace_local: true }
    justification: "Creates the translated document as a new object, leaving the original intact."
min_trust_depth: 1   # process isolation — no native binary, no device access
resource_profile: { vram_gb: 2.5, cpu_cores: 2 }
```

A second Plugin, `com.example.cloud-legal-mt`, registers an `ImplementationDescriptor` under the
*same* `capability_id`, `document.translate.legal`, with `implementation_kind: CloudAPI`, a higher
`quality_profile` score on dense legal terminology, a per-page `CostModel`, and an additional
requested permission, `operation: NetworkEgress(Domain("api.example-legal-mt.com"))`, justified as
"sends document text to our translation service; document is not retained." Because both
manifests' contracts are structurally identical, the Plugin Framework registers the second as an
additional implementation of the same Capability rather than a fork. At invocation time, the
[Model Router](23-multi-model-orchestration.md) is the one that decides between them: for a
document classified `Sensitive` without an active cloud-egress consent record, the privacy gate in
[23](23-multi-model-orchestration.md#architecture) removes the cloud candidate outright and the
local implementation is chosen; if the user has separately consented to cloud translation for legal
documents and the quality gap is material, the cloud implementation may be preferred — exactly the
division of labor this document's Architecture section describes: this document decides *who may
compete*, [23](23-multi-model-orchestration.md) decides *who wins*.

## Security Considerations

Every Plugin, of every contribution kind, runs inside a Trust Boundary per
[03 — Kernel Architecture](03-kernel-architecture.md); there is no ambient-permission contribution
kind — a `KnowledgeProvider` does not implicitly gain filesystem access just because it sounds
read-only. Signature verification and publisher identity binding prevent a tampered package from
being installed under a trusted name; requested-permission-vs-contract cross-checking at manifest
validation time catches over-broad requests before a human ever has to reason about them. Because
capability tokens are minted only for approved grants and are independently enforced by the kernel's
`cap_invoke` (see [03](03-kernel-architecture.md#interfaces--apis)), a Plugin cannot use an
undeclared permission even if its own code attempts to — the enforcement point is outside the
Plugin's Trust Boundary entirely. Supply-chain concerns (a compromised publisher key, a malicious
update) are [17 — Threat Model](17-threat-model.md)'s subject in depth; this document's contribution
is `registry_quarantine`, which disables a Plugin's registry entries and freezes its package for
forensics without requiring an immediate full uninstall.

## Failure Modes

- **Manifest schema mismatch or unsupported `sdk_version`.** Rejected at validation, before any
  Trust Boundary is created.
- **Over-broad permission request.** Rejected automatically; never surfaced to the user as a choice
  to approve, since it is a validation failure, not a consent question.
- **Two Plugins with colliding `capability_id` but incompatible contracts.** Detected by the
  structural compatibility check; resolved by versioned variance rather than silent overwrite.
- **Plugin crash inside its sandbox.** Contained exactly as in
  [03 — Kernel Architecture §Failure Modes](03-kernel-architecture.md#failure-modes) — the Plugin's
  own Trust Boundary absorbs it; no other Plugin or core subsystem observes the fault directly.
- **Malicious Plugin discovered post-install.** Quarantined; its `RegistryEntry` contributions are
  pulled from discovery immediately, degrading gracefully to remaining implementations of the same
  Capability via [23 — Multi-Model Orchestration](23-multi-model-orchestration.md)'s fallback chain.

## Recovery Mechanisms

A crashed Plugin process restarts under the same supervisor-tree pattern as a kernel driver
(see [03 §Recovery Mechanisms](03-kernel-architecture.md#recovery-mechanisms)), re-requesting its
already-approved capability tokens fresh rather than reconciling stale state. Uninstall and
version rollback are both first-class: [33 — Rollback & Recovery](33-rollback-recovery.md) can
restore a previous `PluginManifest` version's registry state exactly, which matters when a Plugin
update silently degrades quality — the previous `ImplementationDescriptor` is not deleted on
upgrade, only superseded, so rollback is a pointer change, not a reinstall.

## Performance Analysis

Manifest validation, signature verification, and Trust Boundary admission are one-time, off the
critical path of any later Capability invocation. The registry lookup that matters on the hot path —
[23 — Multi-Model Orchestration](23-multi-model-orchestration.md)'s per-invocation candidate
gathering — is a cached, exact-key lookup by `capability_id`, deliberately kept O(1)-ish so it never
becomes the routing bottleneck analyzed in [23 §Performance Analysis](23-multi-model-orchestration.md#performance-analysis).
Semantic discovery for the Intent Engine is bounded by
[09 — Knowledge Graph](09-knowledge-graph.md)'s approximate-nearest-neighbor index performance, which
is amortized against Intent parsing, not per-Capability-invocation, latency.

## Trade-offs

Allowing unrestricted competing implementations per Capability maximizes ecosystem openness and
gives the Model Router real choices, but fragments discovery if every minor contract variation mints
a new `capability_id` — Hyperion resolves this by keeping the compatibility bar structural (input,
output, side-effect shape) rather than semantic similarity alone, accepting that some genuinely
near-duplicate Capabilities will register as separate IDs rather than risk silently substituting an
implementation with subtly different guarantees. Requiring every Plugin, however first-party or
trusted-seeming, to sit inside a Trust Boundary costs install-time and runtime overhead a
"blessed core Plugin" exemption would avoid; Hyperion rejects the exemption because it is exactly
the kind of privileged carve-out [02 §5](02-core-architecture.md#5-capability-security-as-the-unifying-security-model)
disallows.

## Testing Strategy

A conformance suite exercises the manifest validator against known-good and adversarial manifests
(over-request, signature mismatch, schema drift) before it is trusted with real installs. Runtime
permission-escalation tests install a Plugin with a deliberately minimal grant set and attempt, from
inside its own Trust Boundary, to invoke operations outside it — these must fail at the kernel's
`cap_invoke`, not merely at this framework's own layer, since defense-in-depth requires the
kernel-level gate to hold even if this document's checks were somehow bypassed. Registry load tests
register hundreds of competing implementations under one popular `capability_id` and verify
discovery and conflict resolution both remain correct and within the latency budget consumed by
[23 — Multi-Model Orchestration](23-multi-model-orchestration.md). The "Translate Legal Documents"
scenario above is maintained as a standing end-to-end test: install both competing Plugins, vary
consent and privacy-tier state, and assert the Model Router's choice matches the expected outcome
under each condition.

---
*Next: [25 — SDK](25-sdk.md).*
