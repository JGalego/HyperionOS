# Privacy Architecture

This document defines how Hyperion decides where computation and storage happen, who may access
what data, and how a user inspects, edits, exports, or erases everything the system remembers. It
is the authority for **data handling, storage, consent, and cross-device confidentiality**. It
does not own capability tokens, sandboxing, or the risk-assessment engine that decides how much
autonomy an action gets — those belong to
[15 — Security Architecture](15-security-architecture.md), and this document composes with it
rather than restating it: Security Architecture decides *whether an action is allowed to run*;
Privacy Architecture decides *where the data may go and who may see it*.

## 1. Purpose

Define the privacy tiers a user can select, the mechanism that makes those tiers a hard
constraint (not a preference) on every routing decision made by
[22 — Local AI Runtime](22-local-ai-runtime.md) and
[23 — Multi-Model Orchestration](23-multi-model-orchestration.md), the encryption model for data
at rest and in multi-device sync, and the concrete API surface by which a user inspects, edits,
exports, or erases anything Hyperion has stored about them — across the
[Memory Engine](08-memory-engine.md) and the full [Knowledge Graph](09-knowledge-graph.md), not
just a subset of it.

## 2. Motivation

An intent-native OS necessarily knows more about a user than any prior operating system — every
goal, every document, every conversation, every calendar event, every half-finished thought
captured as a [Semantic Object](02-core-architecture.md#semantic-object). [01 — Vision &
Philosophy](01-vision-and-philosophy.md) names "Trustworthy with private data by default" as a
success criterion in its own right (§10), not a checkbox. That trust cannot rest on a promise; it
has to rest on a mechanism that is true even when Hyperion's own servers are compromised,
subpoenaed, or simply having a bad day. Three concrete failure patterns motivate this document:

1. **Silent cloud fallback.** A capability that usually runs locally quietly escalates to a cloud
   model when the local one is slow or unavailable, without telling the user. This directly
   violates Design Invariant 3 in [02 — Core Architecture](02-core-architecture.md#4-design-invariants)
   ("local-first by default... never a silent fallback") and is the single failure mode this
   document treats as a Sev-1 defect wherever it is found.
2. **Opaque retention.** A user cannot tell what the system remembers, edit it, or make it
   disappear. This directly violates the brief's requirement that users can "inspect, edit,
   export, or erase everything."
3. **Infrastructure-level access.** Multi-device sync ([21 — Distributed Execution](21-distributed-execution.md))
   moves data through Hyperion-operated relay infrastructure; if that infrastructure can read the
   payload, "private by default" is a policy, not a property.

## 3. Architecture

Privacy is enforced as a **gate that every data-touching and inference-routing decision must pass
through**, not a filter applied after the fact. The Privacy Policy Engine sits logically upstream
of the Model Router, the Plugin Framework, and Multi-Agent Coordination; none of them may bypass
it, because none of them hold key material or consent state directly.

```
                         ┌───────────────────────────────┐
                         │      User Privacy Profile        │
                         │  Fully Local / Local-Preferred-   │
                         │  with-Consent / Cloud-Assisted,   │
                         │  plus per-domain overrides         │
                         └────────────────┬────────────────┘
                                          │ read on every routing / access decision
                ┌─────────────────────────┼─────────────────────────┐
                ▼                         ▼                         ▼
   ┌─────────────────────┐   ┌────────────────────────┐  ┌───────────────────────┐
   │ 22 Local AI Runtime /  │   │ 24 Plugin Framework      │  │ 12 Multi-Agent          │
   │ 23 Model Router        │   │ per-Capability / per-    │  │ Coordination            │
   │  routing decision      │   │ Plugin data-access scope │  │ least-privilege context │
   │  MUST consult tier      │   │                          │  │ sharing between Agents  │
   │  before dispatch        │   │                          │  │                         │
   └───────────┬───────────┘   └────────────┬────────────┘  └────────────┬───────────┘
               │                             │                            │
               ▼                             ▼                            ▼
   ┌─────────────────────────────────────────────────────────────────────────────┐
   │                Consent Ledger — append-only, signed, locally held           │
   └────────────────────────────────────────┬────────────────────────────────────┘
                                             ▼
   ┌─────────────────────────────────────────────────────────────────────────────┐
   │  Data Plane                                                                 │
   │  ┌──────────────────┐   ┌───────────────────────┐   ┌───────────────────┐  │
   │  │ 08 Memory Engine   │   │ 09 Knowledge Graph      │   │ 28 Storage Engine   │  │
   │  │  encrypted at rest │   │  encrypted at rest      │   │  AEAD envelopes     │  │
   │  └──────────────────┘   └───────────────────────┘   └───────────────────┘  │
   │                    Inspect · Edit · Export · Erase API (§6)                 │
   └────────────────────────────────────────┬────────────────────────────────────┘
                                             │ end-to-end encrypted sync envelope
                                             ▼
                         ┌───────────────────────────────────┐
                         │ 21 Distributed Execution              │
                         │ multi-device sync relay — Hyperion's   │
                         │ own infrastructure cannot decrypt the  │
                         │ payload, only route ciphertext         │
                         └───────────────────────────────────┘
```

Every arrow in this diagram crosses a [Trust Boundary](02-core-architecture.md#trust-boundary);
every crossing is capability-secured per [15 — Security Architecture](15-security-architecture.md)
and additionally consent-gated per this document. A component that cannot prove it holds a valid
consent grant for the tier in force is refused, not degraded silently.

## 4. Data Structures

```rust
enum PrivacyTier {
    FullyLocal,               // no network egress for inference or storage, ever
    LocalPreferredWithConsent,// local first; cloud escalation requires a fresh, explicit consent
    CloudAssisted,            // cloud allowed by default for non-sensitive domains; still logged,
                               // still revocable, never for domains marked Restricted (§4)
}

struct PrivacyProfile {
    tier: PrivacyTier,
    domain_overrides: Map<Domain, PrivacyTier>, // e.g. Health, Finance pinned to FullyLocal
                                                 // even under a global CloudAssisted tier
    updated_at: Timestamp,
    version: u32,
}

struct ResidencyTag {                 // attached to every Semantic Object's header (09, 28)
    object_id: ObjectId,
    sensitivity: SensitivityClass,    // Public | Personal | Sensitive | Restricted
    allowed_tiers: Set<PrivacyTier>,  // Restricted objects never carry CloudAssisted
    encryption_key_ref: KeyRef,
}

struct ConsentGrant {
    id: GrantId,
    subject: CapabilityId | PluginId,
    scope: DataScope,          // a Knowledge Graph query, a domain, or a single Object ID
    purpose: String,           // human-readable, shown verbatim at consent time
    expiry: Option<Timestamp>,
    revocable: bool,           // always true
    granted_at: Timestamp,
    proof: Signature,          // signed by the user's device key
}

struct ErasureRequest {
    selector: DataScope,
    mode: SoftDelete | CryptoShred,   // SoftDelete: grace-period tombstone, see 33
    requested_at: Timestamp,
    grace_period: Duration,
}

struct ErasureReceipt {
    object_ids: [ObjectId],
    tombstones: [TombstoneId],
    propagated_to_devices: [DeviceId],
    completed_at: Option<Timestamp>,  // absent until all reachable devices confirm
    verifiable_proof: Hash,
}

struct SyncEnvelope {                 // unit of E2E-encrypted multi-device sync (21)
    object_id: ObjectId,
    ciphertext: Bytes,
    nonce: Bytes,
    aad: Bytes,                       // authenticated but unencrypted routing metadata, minimized
    wrapped_content_key: Map<DeviceId, WrappedKey>,
    sender_device: DeviceId,
    signature: Signature,
}
```

## 5. Algorithms

**Privacy-gated routing.** Every Capability dispatch decision made by
[23 — Multi-Model Orchestration](23-multi-model-orchestration.md), and every model selection made
by [22 — Local AI Runtime](22-local-ai-runtime.md), calls the same gate before it may target a
non-local implementation. The gate is deny-by-default: if tier, residency, or consent state cannot
be established with certainty, the call is refused, not downgraded silently to "best effort."

**Least-privilege context assembly.** When [12 — Multi-Agent Coordination](12-multi-agent-coordination.md)
splits an Intent Graph across Agents, the Context Bundle each Agent receives is computed, not
copied wholesale: for each candidate object, the coordinator checks whether the receiving Agent's
declared sub-intent actually requires it and whether the object's `ResidencyTag` permits the
tier the Agent will run under. Objects that fail either check are excluded, and the Agent is told
what was withheld and why (this feeds directly into
[18 — Explainability & Trust](18-explainability-and-trust.md), which renders that withholding as
part of the reasoning trace so it is never a silent gap).

**Erasure propagation.** Erasure is a cascade, not a single delete: it must remove the object, its
embeddings, every Knowledge Graph edge that references it, every Memory Engine derivative (summaries,
episodic references), and every device's synced copy — including devices that are offline at
request time. This is implemented as a CRDT tombstone so that propagation is commutative and
idempotent: any order of application across any subset of devices converges to the same erased
state, and a device that reconnects after a long absence applies the tombstone before merging any
stale content it is offering back into the mesh.

**Device enrollment and key wrapping.** Adding a device to a user's sync mesh wraps the relevant
per-object content keys for the new device's public key and revokes nothing retroactively (new
devices see only objects synced after enrollment, unless the user explicitly requests full
history). Removing a device rotates the group key for all *future* writes and re-wraps forward;
Hyperion does not attempt cryptographic "unsend" of what the removed device already held, which is
disclosed to the user as a limitation, not hidden.

## 6. Interfaces / APIs

```
privacy.profile.get() -> PrivacyProfile
privacy.profile.set(tier, domain_overrides)

privacy.consent.request(subject, scope, purpose) -> ConsentGrant | Denied
privacy.consent.revoke(grant_id)
privacy.consent.list(subject?) -> [ConsentGrant]

memory.inspect(query) -> [MemoryRecord]              // extends 08's transparency surface
memory.edit(record_id, patch)
memory.export(query, format) -> SignedExportBundle
memory.erase(selector, mode) -> ErasureReceipt       // full form; 08's memory.erase(selector,
                                                       // cascade) is the SoftDelete-default
                                                       // shorthand for this same operation, and
                                                       // 26-apis.md's MemoryEraseRequest is its
                                                       // external HTTP-shaped contract — one
                                                       // erasure operation, three call surfaces

knowledgeGraph.inspect(object_id | query) -> [SemanticObject]  // extends inspect to all of 09
knowledgeGraph.export(scope) -> SignedExportBundle
knowledgeGraph.erase(scope, mode) -> ErasureReceipt

plugin.dataAccess.declare(manifest)                  // install-time declared scope, 24
plugin.dataAccess.request(scope, purpose) -> ConsentGrant | Denied  // runtime, if undeclared

sync.device.enroll(device_id, pairing_proof)
sync.device.revoke(device_id)
sync.key.rotate()
```

`memory.export` and `knowledgeGraph.export` produce a single portable, encrypted bundle — the same
format is used for "take everything with me," honoring user-owned data as a first-class property
rather than a support ticket.

## 7. Pseudocode

`route_capability_call` below illustrates the *policy decision* this document owns — which
privacy tier applies, and whether a remote implementation is admissible at all for this Context
Bundle — collapsed into a single local-or-remote call for clarity. In the real request path this
same decision is what backs [23 — Multi-Model Orchestration](23-multi-model-orchestration.md)'s
per-candidate `privacy_gate` function inside its multi-candidate scoring pipeline (23
§"The privacy gate"), not a second dispatcher competing with 23's `route()`: 23 calls into this
policy for every `ImplementationDescriptor` it considers, and a candidate this function would
refuse is removed from 23's candidate set before scoring, exactly as 23 describes. There is one
routing decision, made by 23, informed at the privacy step by the logic below.

```python
def route_capability_call(capability, intent, context_bundle):
    profile = privacy_profile_of(context_bundle.user)
    domain = classify_domain(intent, context_bundle)
    required_tier = profile.domain_overrides.get(domain, profile.tier)

    local_impl = capability.local_implementation()
    remote_impl = capability.remote_implementation()

    if required_tier == PrivacyTier.FULLY_LOCAL:
        if local_impl is None:
            return degrade_with_disclosure(capability, reason="no local implementation available")
        return dispatch(local_impl)

    if required_tier == PrivacyTier.LOCAL_PREFERRED_WITH_CONSENT:
        if local_impl is not None and meets_quality_bar(local_impl, intent):
            return dispatch(local_impl)
        grant = consent_ledger.find_valid(capability, scope=intent.data_scope)
        if grant is None:
            grant = request_consent(capability, intent.data_scope, purpose=intent.summary)
        if grant is None:                      # user declined
            return degrade_with_disclosure(capability, reason="cloud declined, no local fallback")
        return dispatch(remote_impl, grant=grant)

    # CLOUD_ASSISTED
    if object_residency_forbids(intent.data_scope, PrivacyTier.CLOUD_ASSISTED):
        return dispatch(local_impl) if local_impl else degrade_with_disclosure(capability,
            reason="object marked Restricted; cloud tier not permitted for this data")
    grant = consent_ledger.standing_grant(capability)
    if grant is None:                      # never granted, or since revoked — never assume consent
        return dispatch(local_impl) if local_impl else degrade_with_disclosure(capability,
            reason="no standing cloud consent for this capability (not yet granted, or revoked)")
    return dispatch(remote_impl, grant=grant)

# Invariant checked by CI on every merge: no code path may call `dispatch(remote_impl, ...)`
# without a `grant` object on the call stack. See §13.
```

## 8. Security Considerations

Key material is device-bound and, where hardware allows, backed by a secure element referenced
from [03 — Kernel Architecture](03-kernel-architecture.md); Privacy Architecture never asks
[15 — Security Architecture](15-security-architecture.md) to weaken sandboxing in exchange for
data access — the two gates are independent and both must pass. Sync introduces a metadata side
channel (object sizes, timing, access frequency) even though payloads are opaque; the sync
transport in [21 — Distributed Execution](21-distributed-execution.md) batches and pads envelopes
to reduce it. Erasure itself must not become a signal — a `CryptoShred` tombstone is
indistinguishable on the wire from a routine sync update, so an observer cannot infer that
something sensitive was just deleted. A compromised or over-broad Plugin is a Privacy Architecture
concern only insofar as scoping and consent are concerned; containment of a plugin that ignores its
declared scope is [15 — Security Architecture](15-security-architecture.md)'s risk-assessment
engine's job, invoked the moment this document's consent check reports a violation.

## 9. Failure Modes

- **Consent ledger unreachable.** The system fails closed: no cloud-tier dispatch proceeds without
  a verifiable grant, which means a degraded (local-only) experience rather than a privacy breach.
- **Device offline during erasure.** The tombstone queues and applies on reconnect before any
  content merge, per the CRDT property in §5; the `ErasureReceipt.completed_at` stays unset until
  every last-known device has confirmed, and the user can see which devices are still outstanding.
- **Key loss with no surviving device.** In `FullyLocal` tier, Hyperion holds no escrow key by
  design, so data on a lost, unrecovered device is permanently unrecoverable — this is a stated
  trade-off (§12), not a bug.
- **Plugin scope creep.** A Plugin requesting data outside its declared manifest is denied at the
  gate in §6 and reported to [15](15-security-architecture.md) and
  [34 — Observability & Telemetry](34-observability-telemetry.md); repeated attempts escalate to
  quarantine.
- **Export/erase format drift** across Hyperion versions could make an old export bundle
  unreadable; export bundles are versioned and self-describing to guard against this.

## 10. Recovery Mechanisms

Soft-deletes honor a grace period before cryptographic shredding, integrating with
[33 — Rollback & Recovery](33-rollback-recovery.md) so an accidental erase is itself undoable
until the grace period lapses; only an explicit `CryptoShred` request skips the grace period.
Sync-key loss is mitigated optionally and only by explicit user opt-in into a Shamir secret-sharing
recovery scheme split across the user's own other trusted devices — never a Hyperion-held key,
preserving the "even Hyperion's own infrastructure can't read it" property. The erasure retry queue
is idempotent, so a crashed or partially-applied propagation simply resumes from the last
confirmed device without risk of double-application or partial state.

## 11. Performance Analysis

AEAD encryption (AES-256-GCM or ChaCha20-Poly1305, hardware-accelerated) adds under 5% CPU
overhead on the [28 — Storage Engine](28-storage-engine.md) read/write path — effectively free
relative to inference latency. The consent gate targets under 10 ms for a cached grant lookup and
should never appear on the interactive path for `FullyLocal` sessions, since no remote call is
attempted at all. Sync bandwidth is dominated by content, not key material: group-key wrapping
means re-wrap cost is O(devices), triggered only on enrollment/revocation, not O(objects) per sync
tick. Erasure propagation convergence time across N devices is bounded by the sync system's gossip
fanout in [21](21-distributed-execution.md); typical convergence for an actively-connected mesh is
within one sync interval, with stragglers converging on reconnect.

## 12. Trade-offs

`FullyLocal` strictness can collide with Design Invariant 5 ("degrade, never fail closed on the
user's goal") when only a frontier cloud model can competently satisfy an Intent — Hyperion
resolves this by degrading capability quality and *disclosing* the degradation
(`degrade_with_disclosure`), never by silently loosening the privacy tier. Per-call consent
granularity is the most privacy-preserving option but risks prompt fatigue; Hyperion mitigates this
with scoped, remembered grants that expire and periodically re-confirm rather than re-prompting on
every invocation. End-to-end encrypted sync forecloses server-side search indexing, so all
semantic search and Knowledge Graph queries in [09](09-knowledge-graph.md) must be computed
client-side against locally decrypted data — a deliberate cost accepted in exchange for the
"Hyperion's own infrastructure can't read it" guarantee. Finally, a permissive `CloudAssisted`
default would improve capability breadth for the median user, but the specification chooses
`LocalPreferredWithConsent` as the recommended default (with `FullyLocal` and `CloudAssisted` as
explicit opt-ins) to keep the OS's default posture trustworthy without requiring configuration.

## 13. Testing Strategy

CI enforces a static/dynamic check that no `dispatch(remote_impl, ...)` call exists on any path
lacking a `grant` in scope (the invariant noted in §7), turning "no silent cloud fallback" into a
build-breaking regression rather than a design aspiration. Erasure completeness is fuzz-tested
against generated Knowledge Graph fixtures, asserting no dangling edges, embeddings, or Memory
Engine derivatives survive a full erase. A multi-device simulation harness exercises sync
convergence and tombstone propagation under network partition and device churn. Consent
expiry/revocation has a dedicated regression suite, and export/import round-trips are tested for
fidelity (export, erase, reimport must fully restore pre-erasure state; export, erase, and *not*
reimporting must leave verifiably no trace). See also [35 — Testing Strategy](35-testing-strategy.md)
for how these suites integrate into the overall test pyramid, and
[17 — Threat Model](17-threat-model.md) for the adversarial scenarios these mechanisms are tested
against.

---
*Next: [17 — Threat Model](17-threat-model.md).*
