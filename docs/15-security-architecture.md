# Security Architecture

This document is the cross-layer authority for how capability security — introduced as Hyperion's
single, unifying security model in [02 — Core Architecture](02-core-architecture.md#5-capability-security-as-the-unifying-security-model)
and enforced at the boundary in [03 — Kernel Architecture](03-kernel-architecture.md) — composes
across every layer above the kernel. It specifies how a **capability token** is minted from a
user's [Intent](05-intent-engine.md), delegated to an [Agent](11-agent-runtime.md), attenuated
again to a [Plugin](24-plugin-framework.md), and re-checked at the final system call; how
[Sandboxing](03-kernel-architecture.md) is enforced on Agents and Plugins; how
[IPC](30-ipc-framework.md) between processes and devices is mutually authenticated; and how the
"Safety" principle from [01 — Vision & Philosophy](01-vision-and-philosophy.md#9-human-control-is-non-negotiable)
is implemented as a concrete **Risk Assessment Engine** that replaces uniform "Are you sure?"
dialogs with graduated, intent-aware intervention. See
[16 — Privacy Architecture](16-privacy-architecture.md) for *what* data may be seen at all, and
[17 — Threat Model](17-threat-model.md) for the specific attacks this architecture defends against.

## 1. Purpose

To define, precisely enough to implement, the one security model referenced throughout this
specification: capability tokens as the sole unit of authority; their lifecycle (mint, delegate,
attenuate, revoke) as authority flows from a human Intent down through Agents and Plugins to a
kernel system call; the enforcement mechanics of sandboxing and secure IPC that make that lifecycle
tamper-resistant; and the Risk Assessment Engine that decides, for every autonomous state-changing
action, how much friction a human should experience before it happens.

## 2. Motivation

Traditional operating systems enforce security with three uncoordinated models stacked on top of
each other: a kernel permission model, an application permission-dialog model, and — once cloud
services enter the picture — a separate IAM/OAuth model that neither of the first two understands.
Each boundary is a place where authority can be misrepresented, and each dialog asks the same
binary question regardless of what is actually at stake, producing the well-documented failure
mode of users reflexively clicking "Allow" on everything. This is incompatible with Hyperion's
[Golden Rule](01-vision-and-philosophy.md#2-the-golden-rule): a security model that trains users to
stop reading prompts is not making goals easier to accomplish safely, and a system that is
proactive on the user's behalf (per [01 §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable))
needs a security model expressive enough to distinguish "rename a file" from "delete every family
photo from the last five years." Hyperion instead has exactly one security primitive — the
capability token — used identically at every layer boundary in [02 §1](02-core-architecture.md#1-layered-system-view),
and exactly one graduated-response mechanism for autonomous risk, described below.

## 3. Architecture

CapabilityGrants flow downward from an Intent, narrowing at every hop, with a cheap pre-flight
`capability_check` at each holder; the Risk Assessment Engine intercepts state-changing invocations
before they reach the kernel; the kernel's redemption-time check (03 `cap_derive`/`cap_invoke`) is
the only place authority is ever actually granted.

```
 L6 Experience     "Delete last month's family photos" (user Intent, 05-intent-engine.md)
                                     │
 L5 Coordination    ┌────────────────────────────────────────────────┐
                     │   Risk Assessment Engine (§7 below)            │
                     │   blast-radius · reversibility · sensitivity · │
                     │   confidence · corroboration → intervention   │
                     └───────────────────┬────────────────────────────┘
                                         │ intervention level decided
                                         ▼
 L4 Cognition       Agent Runtime (11-agent-runtime.md) holds root grant
                     Ct0 = {subject: PhotoCleanupAgent,
                            object: class(photo, tag=family, month=June),
                            ops: {read, delete}, ttl: 5m, issuer: kernel}
                                         │ delegate + attenuate (§5.2, offline)
                                         ▼
 L2 Platform        Plugin "gallery.declutter" (24-plugin-framework.md)
                     holds Ct1 = attenuate(Ct0, ops:{delete}, ttl:1m)
                     runs capability_check(Ct1, ...) pre-flight (§7)
                                         │ IPC call, mutually authenticated
                                         │ (30-ipc-framework.md, §5.6)
                                         ▼
 L1 System Runtime  Sandboxed Plugin process — kernel-enforced syscall
                     filter per provenance tier (03-kernel-architecture.md)
                                         │ syscall: unlink(object_ref) + Ct1
                                         ▼
 L0 Kernel          Ct1 redeemed into a live 03 CapabilityToken via
                     cap_derive, then cap_invoke(...) — the ONE
                     enforcement gate. Independently re-verifies chain
                     Ct1→Ct0→root against the kernel's own registry: not
                     revoked, not expired, attenuation-only  →  ALLOW / DENY
                                         │
                          ┌──────────────┴───────────────┐
                          ▼                               ▼
                Audit log entry                  Recovery point created
          (18-explainability-and-trust.md)     (33-rollback-recovery.md)
```

Every hop in this diagram — Agent-to-Plugin delegation, cross-Agent handoffs in
[12 — Multi-Agent Coordination](12-multi-agent-coordination.md), and cross-device calls in
[21 — Distributed Execution](21-distributed-execution.md) — is ultimately settled by the identical
kernel-level `cap_invoke` check defined in [03 — Kernel Architecture](03-kernel-architecture.md).
There is no second, faster, less-checked path; the
[Trust Boundary](02-core-architecture.md#trust-boundary) definition ("no implicit authority
crosses") holds everywhere, not just at the process/kernel edge. This document adds exactly one
layer on top of that kernel primitive, described next — it does not define a second, competing
kernel-equivalent primitive.

**Two tiers, one primitive.** [03 — Kernel Architecture](03-kernel-architecture.md) defines the
actual enforcement primitive — a minimal `CapabilityToken` (`object_id`, `rights`, `generation`,
`origin`, `expiry`) that `cap_invoke` checks in O(1) against the live registry generation on every
syscall. That struct is deliberately too small to carry the provenance, delegation history, or
policy metadata the rest of this document needs. Everything above the kernel boundary — Agents,
Plugins, cross-device calls — instead holds a **CapabilityGrant**: a richer, subject-facing
envelope that wraps *one* underlying kernel `CapabilityToken` and carries the audit trail needed to
reason about it. A CapabilityGrant is never itself the thing `cap_invoke` checks; at the moment a
subject actually calls into the kernel, its runtime **redeems** the grant's current (most narrowed)
claim into a live kernel `CapabilityToken` via `cap_derive` (03), and it is that kernel-minted
token — not the CapabilityGrant's self-reported `delegation_chain` — that `cap_invoke` validates
against the kernel's own registry. `capability_check` (§7) is therefore a **pre-flight** check a
subject's runtime runs against its own CapabilityGrant, cheaply rejecting a malformed or
inconsistent grant with a clear reason before ever attempting redemption; it is not itself the
security boundary. The kernel's independent re-validation at redemption time is what actually
prevents a forged or tampered grant from being exploited (see §8) — `capability_check` failing to
catch something is a bug worth fixing, but it is never the last line of defense.

## 4. Data Structures

- **CapabilityGrant** — `token_id`, `subject_id`, `issuer_id`, `object_ref` (a Semantic Object ID
  or a typed pattern over the [Knowledge Graph](09-knowledge-graph.md)), `operations` (a set drawn
  from a fixed verb vocabulary: read, write, delete, execute, network, share), `constraints`
  (`ttl_expiry`, `rate_limit`, `device_binding`, `channel_binding_id`), `delegation_chain` (ordered
  attenuation records back to a kernel-minted root), `trust_tier_ceiling`, `signature`,
  `kernel_token_ref` (the live [03] `CapabilityToken` this grant currently redeems into, once
  redemption has happened at least once; `None` before first use).
- **AttenuationRecord** — `parent_token_id`, `narrowed_operations`, `narrowed_object_ref`,
  `narrowed_constraints`, `delegator_signature`. Appended, never rewritten, each time a grant is
  narrowed; each record stores only the *result* of that narrowing step (never a snapshot of the
  parent's own claim — see §7's `capability_check` for how this is verified without that snapshot).
- **SandboxProfile** — `subject_id`, `provenance_tier` (0 first-party/kernel-signed, 1 vetted
  marketplace, 2 unvetted community — monotonically decreasing trust, directly corresponding to
  [11 — Agent Runtime](11-agent-runtime.md)'s `system` / `verified` / `community` manifest tiers),
  `dev_unsandboxed` (bool, default false; true only on internal developer builds gated by a
  build-time flag unavailable in production — orthogonal to `provenance_tier`, never a "higher
  tier" a production Plugin or Agent can reach), `allowed_syscalls`, `resource_quota`,
  `network_egress_policy`, `filesystem_view` (compatibility view per
  [10 — Semantic Filesystem](10-semantic-filesystem.md)). Code that fails vetting entirely — no
  `provenance_tier` can be assigned — is [11]'s `untrusted` case: it is never spawned as a native
  Agent/Plugin subject at all, and can only run, if at all, fully mediated and opaque under
  [27 — Compatibility Layer](27-compatibility-layer.md)'s Trust Boundary, which does not consult
  `SandboxProfile` in the first place.
- **RiskAssessment** — `action_id`, `blast_radius_score`, `reversibility_score`,
  `sensitivity_score`, `confidence_score`, `corroboration_score`, `composite_score`,
  `intervention_level`, `rationale` (feeds [18 — Explainability & Trust](18-explainability-and-trust.md)),
  `recovery_point_ref` (feeds [33 — Rollback & Recovery](33-rollback-recovery.md)).
- **RevocationEntry** — `subject_id | token_id`, `epoch`, `cascade: bool`, `reason`.
- **IPCSession** — `session_id`, `endpoint_a_identity`, `endpoint_b_identity`, `session_key`,
  `channel_binding_id`, `established_at`, `expiry`.

## 5. Algorithms

**5.1 Minting.** Root CapabilityGrants are minted only by the kernel — via 03's `cap_derive` from a
freshly `device_claim`/`sandbox_create`-minted kernel `CapabilityToken` — in direct response to
either an explicit, auditable user grant traced to an Intent node, or a first-party policy fixed at
install time. No process outside the kernel can mint a root grant; this is the anchor of the
entire chain.

**5.2 Delegation and attenuation.** CapabilityGrants are structured so any holder can derive a
narrower child grant *offline*, without contacting the kernel, by appending an `AttenuationRecord`
and re-signing a hash chain over it (a macaroon-style construction). This lets an Agent delegate to
a Plugin, or a Plugin to a sub-capability, without a redemption round-trip on every hop — the round
trip only has to happen when the grant is actually redeemed into a live kernel token at invocation
time (§3). The invariant enforced — first as a cheap pre-flight in `capability_check` (§7), then
authoritatively by the kernel at redemption via `cap_derive` (03) — is **attenuation-only**: a
derived grant's `operations`, `object_ref` scope, and `ttl` must each be a subset of its parent's,
and its effective authority can never exceed the subject's `trust_tier_ceiling` regardless of what
a delegator claims (see [17 — Threat Model](17-threat-model.md), mitigation for malicious Plugins).

**5.3 Revocation.** Revocation is push-based (an immediate broadcast to any session holding the
affected subtree) backed by epoch counters for the offline case: every grant carries the epoch it
was issued under, and both `capability_check`'s pre-flight and the kernel's redemption-time
recheck reject any grant whose epoch predates a subject's current revocation epoch — the kernel
recheck is additionally backed by 03's `generation` counter on the underlying token, so a
redemption is refused even if a stale grant's epoch field were somehow forged. Revoking a grant
cascades to every grant attenuated from it. High-risk operation classes default to short TTLs
specifically so an offline or partitioned holder's exposure window is bounded even if the push
never arrives.

**5.4 Risk scoring.** See §7 for the full algorithm: a weighted composite over four axes, mapped to
one of four intervention levels, with a hard floor that overrides the score whenever an action's
justification is provenance-tainted (traced to ingested content rather than a user-originated
Intent node — see [17 — Threat Model](17-threat-model.md) mitigation #1).

**5.5 Sandbox enforcement.** Every subject (Agent or Plugin) is assigned a `SandboxProfile` at
spawn time by the `provenance_tier` of its Capability, per
[03 — Kernel Architecture](03-kernel-architecture.md). The syscall allowlist is a *projection* of
`provenance_tier`, computed by the kernel — never something a Plugin manifest can widen by
declaring more permissions than its tier allows, and never something `dev_unsandboxed` can grant
outside of an internal developer build.

**5.6 IPC mutual authentication.** Every process pair and every device pair establishes an
`IPCSession` via a handshake (Noise-protocol-class: mutual identity proof, ephemeral forward-secure
session key, `channel_binding_id` derived from the session). CapabilityGrants (and the kernel
tokens they redeem into) are bound to the `channel_binding_id` of the session they were delegated
over; a grant intercepted or replayed on a different channel fails the bind check and is rejected,
closing the classic token-theft replay path across the
[Device Framework](20-device-framework.md) and [Distributed Execution](21-distributed-execution.md)
boundary.

## 6. Interfaces / APIs

```
CapabilityService.mint(subject, object_ref, ops, constraints, justification_intent_id)
    -> CapabilityGrant
CapabilityService.delegate(parent_grant, narrow_ops, narrow_object, narrow_constraints)
    -> CapabilityGrant
CapabilityService.check(grant, requested_op, requested_object, channel_binding_id)
    -> ALLOW | DENY(reason)                          # pre-flight only, see §3 "Two tiers"
CapabilityService.redeem(grant) -> CapabilityToken   # -> 03 cap_derive; kernel is authoritative
CapabilityService.revoke(token_id_or_subject_id, cascade: bool, reason) -> RevocationEntry

RiskAssessmentEngine.assess(pending_action) -> RiskAssessment
RiskAssessmentEngine.register_signal(source, weight)   # extensibility hook for new corroboration
                                                         # sources, e.g. a new backup provider

SandboxManager.spawn(subject, provenance_tier) -> SandboxHandle
IPCChannel.open(local_identity, remote_identity) -> IPCSession
```

## 7. Pseudocode

```python
def capability_check(grant, requested_op, requested_object, channel_binding_id):
    """Pre-flight check a subject's runtime runs against its own CapabilityGrant before
    attempting kernel redemption (03 cap_derive/cap_invoke) — see §3 "Two tiers, one primitive".
    This is NOT the security boundary: it cheaply rejects a malformed or self-inconsistent grant
    with a clear reason, but the kernel independently re-validates at redemption time against its
    own live registry, which is what actually stops a forged or tampered grant (§8)."""
    if grant.constraints.channel_binding_id != channel_binding_id:
        return DENY("grant not bound to this channel")
    if now() > grant.constraints.ttl_expiry:
        return DENY("grant expired")
    if is_revoked(grant.token_id, grant.delegation_chain):
        return DENY("grant or an ancestor was revoked")

    # Internal chain-consistency check: each attenuation step's narrowed claim must nest
    # inside the step before it. AttenuationRecord only stores the *result* of a narrowing
    # step (never a snapshot of the parent's own claim), so this walks outward from the
    # grant's own current (narrowest) operations/object_ref toward the kernel-minted root,
    # comparing each record against the one before it rather than against undefined
    # "parent" fields.
    bound_operations, bound_object_ref = grant.operations, grant.object_ref
    for record in grant.delegation_chain:
        if not is_subset(bound_operations, record.narrowed_operations):
            return DENY("attenuation violated: operations expanded")
        if not scope_within(bound_object_ref, record.narrowed_object_ref):
            return DENY("attenuation violated: object scope expanded")
        bound_operations, bound_object_ref = record.narrowed_operations, record.narrowed_object_ref
    # bound_operations/bound_object_ref now hold the root's originally-claimed bound; the
    # kernel independently re-validates this against its own live registry at redemption
    # time (§3), since only the kernel can attest the root claim is still current.

    if grant.trust_tier_ceiling > subject_ceiling(grant.subject_id):
        return DENY("exceeds subject's kernel-assigned trust ceiling")
    if requested_op not in grant.operations or not covers(grant.object_ref, requested_object):
        return DENY("operation or object outside grant scope")
    return ALLOW


def risk_assess(action):
    blast   = score_blast_radius(action.object_refs, action.scope)       # 0..1
    revers  = score_reversibility(action, recovery_index=ROLLBACK_INDEX) # 0..1, 1 = trivially undoable
    sensit  = score_sensitivity(action.object_refs, classifier=PRIVACY_CLASSIFIER)  # 0..1
    conf    = action.intent_engine_confidence                            # 0..1
    corrob  = score_corroboration(action)  # e.g. recent verified backup, consistent history

    composite = (0.30 * blast
                 + 0.25 * (1 - revers)
                 + 0.20 * sensit
                 + 0.15 * (1 - conf)
                 - 0.10 * corrob)

    if is_provenance_tainted(action):        # see 17-threat-model.md, mitigation #1
        floor = REQUIRE_EXPLICIT_CONFIRM
    elif revers <= 0.05 and blast >= 0.8:
        # A near-fully-irreversible, wide-blast-radius action always gets a guaranteed
        # recovery point, full stop — corroboration and confidence are not allowed to buy
        # this down. Without this floor, corroboration's small weight (-0.10) can pull an
        # otherwise maximal-risk action (blast=1, revers=0, sensit=1, conf=1) just under the
        # require-backup-first threshold (composite=0.74 with corrob=0.1), which would let
        # exactly the class of action Design Invariant 2 exists to protect skip its
        # guaranteed synchronous checkpoint. This floor closes that gap unconditionally.
        floor = REQUIRE_BACKUP_FIRST
    else:
        floor = SILENT_PROCEED

    level = max(floor, level_for_score(composite))
    # thresholds: <0.2 silent-proceed | <0.45 notify-and-proceed |
    #             <0.75 require-explicit-confirm | else require-backup-first

    if level >= REQUIRE_BACKUP_FIRST:
        recovery_point = create_recovery_point(action)   # 33-rollback-recovery.md

    return RiskAssessment(
        action_id=action.id, blast_radius_score=blast, reversibility_score=revers,
        sensitivity_score=sensit, confidence_score=conf, corroboration_score=corrob,
        composite_score=composite, intervention_level=level,
        rationale=explain(blast, revers, sensit, conf, corrob),   # 18-explainability-and-trust.md
        recovery_point_ref=recovery_point if level >= REQUIRE_BACKUP_FIRST else None,
    )
```

The four intervention levels are deliberately not four flavors of the same dialog:
**silent-proceed** logs and continues; **notify-and-proceed** surfaces a passive, dismissable
notice while the action runs; **require-explicit-confirm** blocks and presents the `rationale` —
what will change, why the system believes it matches the user's goal, and what the alternative is
— generated per [18 — Explainability & Trust](18-explainability-and-trust.md); **require-backup-first**
additionally blocks on a completed recovery point from
[33 — Rollback & Recovery](33-rollback-recovery.md) before execution proceeds, so "delete family
photos" produces a backup, an undo point, and a plain-language explanation instead of a single
"Are you sure?" toggle.

## 8. Security Considerations

The attenuation-only invariant (§5.2) is the mechanism that prevents a confused-deputy attack: no
delegate can ever hold more authority than its delegator, and the kernel — not the delegator —
verifies this at redemption time, not `capability_check`. 03's `cap_invoke` is the single
enforcement point referenced in every layer of [02 §1](02-core-architecture.md#1-layered-system-view);
no subsystem document in this specification may define a path to a kernel object, or a path to
minting/redeeming a CapabilityGrant, that bypasses it — including cross-Agent handoffs
([12](12-multi-agent-coordination.md)) and cross-device calls
([21](21-distributed-execution.md)) — this is what guarantees no subsystem is a lower-security
backdoor into the graph. `CapabilityService.check`'s pre-flight rejection is a usability and
defense-in-depth measure (fail fast, with a clear reason, before a round trip); it is never
credited as the reason an attack is prevented. The kernel's root signing key is hardware-backed
where available (secure
enclave / TPM-class root of trust, per [03 — Kernel Architecture](03-kernel-architecture.md)) since
its compromise would compromise every token in the system. The Risk Assessment Engine's inputs are
sourced from verified system facts (an actual recovery-point record, an actual backup-completion
event) wherever possible rather than solely from a model's self-reported confidence, because the
model reasoning about the action must not also be the uncontested judge of the action's risk (see
[17 — Threat Model](17-threat-model.md) mitigation #8).

## 9. Failure Modes

- **Risk Assessment Engine unavailable or times out.** Fail-safe default is the minimum of
  `require-explicit-confirm` — the engine's absence never degrades to `silent-proceed`.
- **Revocation propagation delay** to an offline or partitioned device; bounded by short default
  TTLs on high-risk operation classes (§5.3).
- **Clock skew** causing premature or delayed token expiry across federated devices; mitigated by
  NTP-disciplined monotonic clocks and a tolerance window checked at `capability_check`.
- **Sandbox escape attempt** from a Tier 2/3 Plugin; caught by the kernel syscall filter, not by
  Plugin-declared intent.
- **IPC session hijack** attempted against a stale or predicted session key; mitigated by
  forward-secure ephemeral keys and channel binding (§5.6).
- **Delegation-chain exhaustion**, an attempted denial-of-service via unbounded attenuation depth;
  the kernel enforces a maximum chain depth (§11).

## 10. Recovery Mechanisms

A compromised or misbehaving subject (Agent or Plugin) can be frozen instantly via
`CapabilityService.revoke(subject_id, cascade=true)`, invalidating its entire delegation subtree in
one call. Any action gated at `require-backup-first` has a ready-made recovery point to roll back
to via [33 — Rollback & Recovery](33-rollback-recovery.md). Every `ALLOW`/`DENY` decision and every
`RiskAssessment` is written to the audit log consumed by
[18 — Explainability & Trust](18-explainability-and-trust.md) and
[34 — Observability & Telemetry](34-observability-telemetry.md), enabling forensic reconstruction of
exactly which delegation chain authorized a disputed action after the fact.

## 11. Performance Analysis

Chain verification in `capability_check` is O(depth); depth is bounded (default maximum 8 hops) so
worst-case verification is a small constant. Verified chains are cached until near their expiry to
avoid re-walking on every call. Revocation checks are O(1) via epoch counters (with a bloom filter
fast-path for cascade sets). The Risk Assessment Engine targets a sub-50ms budget for the
`silent-proceed`/`notify-and-proceed` path so it never becomes a perceptible tax on responsiveness,
consistent with the latency targets in [36 — Performance Benchmarks](36-performance-benchmarks.md);
`require-backup-first` is explicitly allowed to take longer, since a completed recovery point is a
correctness precondition, not a latency target.

## 12. Trade-offs

Offline, macaroon-style attenuation (§5.2) trades instant full revocability for delegation that
doesn't require a kernel round-trip on every hop; Hyperion resolves this by pairing it with short
TTLs and push revocation specifically on high-risk operation classes, accepting slightly more
frequent re-delegation there in exchange for a bounded exposure window. Re-checking capability at
every layer (rather than once at the top) costs some latency but is kept because it is what makes
"no single subsystem is a backdoor" true rather than aspirational — this is worth the cost per the
[Golden Rule](01-vision-and-philosophy.md#2-the-golden-rule)'s trust requirement. Graduated,
intent-aware intervention is richer and more relevant than uniform confirmation dialogs but
introduces calibration risk (a miscalibrated engine could under- or over-intervene); this is
mitigated by making every score component explainable (§7's `rationale`) and by treating any
personalization of thresholds as a low-weight signal, since a poisoned "the user always approves
this" memory would otherwise be a direct lever on intervention level (see
[17 — Threat Model](17-threat-model.md), memory poisoning). "Low-weight" alone is not sufficient,
though: at `corroboration`'s nominal weight (-0.10), it can still tip a near-maximal-risk action
(high blast radius, high sensitivity, low confidence-adjusted-down) just under the
`require-backup-first` threshold. The unconditional irreversibility-and-blast-radius floor in §7's
`risk_assess` (`revers <= 0.05 and blast >= 0.8 → REQUIRE_BACKUP_FIRST`) is what actually closes
that gap: it is a hard floor no score component, corroboration included, can buy down, so a
poisoned corroboration signal can at most affect *how much friction beyond the guaranteed minimum*
an irreversible, high-blast-radius action gets — never whether it gets a recovery point at all.

## 13. Testing Strategy

Property-based testing verifies the attenuation-only invariant holds for arbitrarily generated
delegation chains (no generated chain should ever pass `capability_check` with expanded authority).
A red-team corpus of destructive-action scenarios calibrates and regression-tests Risk Assessment
Engine thresholds, maintained alongside [35 — Testing Strategy](35-testing-strategy.md). Chaos tests
simulate network partitions to verify revocation propagation bounds hold. IPC fuzzing and replay
simulations validate channel binding rejects off-channel tokens. A dedicated sandbox-escape suite,
run against every Plugin trust tier, is a release gate for [24 — Plugin Framework](24-plugin-framework.md).

---
*Next: [16 — Privacy Architecture](16-privacy-architecture.md).*
