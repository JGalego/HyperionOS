# APIs

## Purpose

This document specifies the concrete, system-facing API surface that every other subsystem's
pseudocode calls into: the **Intent API**, the **Context API**, the **Memory API**, the
**Knowledge Graph API**, and the **Capability Invocation API**. It is the layer
[25 — SDK](25-sdk.md)'s tooling is built against, the layer [24 — Plugin Framework](24-plugin-framework.md)'s
Capabilities call at runtime, and the layer every worked trace in
[02 §3](02-core-architecture.md#3-how-a-request-flows-through-the-layers) implicitly invokes at
each step. This document does not redefine the Intent lifecycle, Context Bundle propagation rules,
Memory decay model, or Knowledge Graph schema themselves — those are owned by
[05](05-intent-engine.md), [06](06-context-engine.md)/[07](07-context-propagation.md),
[08](08-memory-engine.md), and [09](09-knowledge-graph.md) respectively. It owns the **wire
contract**: request/response schemas, authentication and authorization, and the invocation model
that lets those subsystems be called uniformly from anywhere in the stack.

## Motivation

[02 §5](02-core-architecture.md#5-capability-security-as-the-unifying-security-model) requires
"exactly one security model" enforced at the kernel boundary and re-checked above it — not one
model per subsystem. Without a single, canonical API layer, each of the Intent Engine, Context
Engine, Memory Engine, Knowledge Graph, and Capability Registry would be tempted to invent its own
bespoke request format and its own ad hoc authorization check, and the "one security model"
invariant would quietly become five. This document exists so that an [Agent](02-core-architecture.md#agent),
a [Capability](02-core-architecture.md#capability) implementation, or an external
[SDK](25-sdk.md) tool all speak the same five APIs, authenticated the same way, regardless of
which subsystem answers the call — and so that [08 — Memory Engine](08-memory-engine.md)'s
transparency requirement (a user can always see, export, or erase what Hyperion remembers) has a
concrete, enforceable API surface rather than being a property asserted only in prose.

## Architecture

The API layer is a thin, uniform gateway in front of five subsystem servers. It performs
authentication and authorization once, at the edge, using the same capability-token primitive
defined at the kernel ([03 — Kernel Architecture](03-kernel-architecture.md#capability-security-as-the-kernel-primitive))
and policed by [15 — Security Architecture](15-security-architecture.md), then routes the
validated request to the owning subsystem over the IPC substrate in
[30 — IPC Framework](30-ipc-framework.md).

```
┌───────────────────────────────────────────────────────────────────────┐
│ CALLERS: Agents (11) · Capability implementations (24) · SDK tools (25)│
└───────────────────────────────────┬───────────────────────────────────┘
                                     │  API Token (§Data Structures)
                                     ▼
┌───────────────────────────────────────────────────────────────────────┐
│                       API GATEWAY (this document)                     │
│  ┌─────────────────────────────────────────────────────────────────┐  │
│  │ Token verification · scope check · rate/quota · request schema  │  │
│  │ validation (15-security-architecture.md, 03-kernel-architecture) │  │
│  └───────────────────────────┬─────────────────────────────────────┘  │
│                               ▼  routed by declared scope               │
│  ┌───────────┐ ┌───────────┐ ┌───────────┐ ┌───────────┐ ┌──────────┐ │
│  │ Intent    │ │ Context   │ │ Memory    │ │ Knowledge │ │Capability│ │
│  │ API       │ │ API       │ │ API       │ │ Graph API │ │Invocation│ │
│  └─────┬─────┘ └─────┬─────┘ └─────┬─────┘ └─────┬─────┘ └────┬─────┘ │
└────────┼─────────────┼─────────────┼─────────────┼────────────┼───────┘
         ▼             ▼             ▼             ▼            ▼
   05-intent-      06/07-context-  08-memory-   09-knowledge-  23-multi-
   engine.md       {engine,prop}   engine.md    graph.md       model-
                                                                orchestration.md
```

Each of the five APIs below is a thin, typed contract in front of the subsystem that actually
implements it; this document defines only what crosses the gateway, not how the subsystem answers
internally.

**Intent API** — submit, query, and cancel an Intent, backing [05 — Intent Engine](05-intent-engine.md).
**Context API** — fetch a Context Bundle synchronously or subscribe to its changes, backing
[06 — Context Engine](06-context-engine.md) and [07 — Context Propagation](07-context-propagation.md).
**Memory API** — read, write, decay, export, and erase, backing [08 — Memory Engine](08-memory-engine.md)'s
transparency requirement directly.
**Knowledge Graph API** — query, traverse, and write Semantic Objects, backing
[09 — Knowledge Graph](09-knowledge-graph.md).
**Capability Invocation API** — invoke-by-contract, letting
[23 — Multi-Model Orchestration](23-multi-model-orchestration.md) resolve which Implementation
actually runs.

## Data Structures

```typescript
// The bearer credential every API call presents. Minted by the kernel's
// capability monitor (03-kernel-architecture.md) and re-checked at the gateway,
// not a separate identity system — this is what keeps "one security model" true.
interface ApiToken {
  tokenId: string;
  boundary: TrustBoundaryId;         // which Trust Boundary this was minted for
  scopes: Scope[];                   // e.g. "intent:submit", "memory:erase"
  subjectObjectIds?: string[];       // narrows scope to specific Semantic Objects
  expiry: string;                    // ISO 8601; short-lived by default
  signature: Bytes;                  // signed by the capability monitor
}

// ---- Intent API ------------------------------------------------------
interface SubmitIntentRequest {
  utterance?: string;                // raw natural-language goal, or:
  structuredGoal?: IntentGraphFragment;
  contextBundleRef?: ContextBundleId;
  priority: "interactive" | "background";
}
interface SubmitIntentResponse {
  intentId: string;
  status: "proposed" | "planned" | "executing";
  subIntents: IntentSummary[];       // per 05's Intent Graph decomposition
}

// ---- Context API -------------------------------------------------------
interface ContextBundleRequest {
  scope: "intent" | "agent" | "session";
  refId: string;                    // intentId, agentInvocationId, or sessionId
  maxAgeMs?: number;                // staleness tolerance for a cached bundle
}
interface ContextBundleResponse {
  bundleId: string;
  activeObjects: SemanticObjectRef[];
  recentIntents: IntentSummary[];
  relevantMemory: MemoryFragmentRef[];
  deviceIdentity: DeviceRef;
  generatedAt: string;
}

// ---- Memory API ----------------------------------------------------
interface MemoryWriteRequest {
  kind: "episodic" | "procedural" | "semantic-preference";
  content: unknown;
  provenance: { intentId?: string; agentId?: string };
  decayPolicy: "standard" | "session-only" | "pinned";
}
interface MemoryExportResponse {
  subjectUserId: string;
  records: MemoryRecord[];          // full, human-readable, per 08's transparency requirement
  format: "json" | "human-readable-report";
}
interface MemoryEraseRequest {
  selector: { recordIds?: string[]; before?: string; kind?: MemoryWriteRequest["kind"] };
  cascade: boolean;                 // also erase derived Knowledge Graph edges; default true,
                                     // matching 08-memory-engine.md's memory.erase default
  mode?: "SoftDelete" | "CryptoShred"; // optional; defaults to SoftDelete. Same operation and
                                        // same ErasureReceipt as 16-privacy-architecture.md's
                                        // memory.erase(selector, mode) — this is its HTTP-shaped
                                        // external contract, not a fourth, separate operation.
}
interface MemoryEraseResponse {         // == 16's ErasureReceipt, JSON-shaped
  objectIds: string[];
  tombstones: string[];
  propagatedToDevices: string[];
  completedAt: string | null;           // absent until all reachable devices confirm
}

// ---- Knowledge Graph API -----------------------------------------------
interface KgQueryRequest {
  query: string;                     // semantic query, not a path
  filters?: { type?: string; relation?: string };
  maxResults: number;
}
interface KgTraverseRequest {
  fromObjectId: string;
  relations: string[];
  maxDepth: number;                  // bounded; see §Algorithms
}
interface KgWriteRequest {
  object: SemanticObjectDraft;
  relations: { type: string; toObjectId: string }[];
}

// ---- Capability Invocation API ------------------------------------------
interface InvokeRequest {
  contractId: string;                // e.g. "legal.translate" (25-sdk.md §Data Structures)
  inputs: Record<string, unknown>;
  contextBundleRef: ContextBundleId;
  constraints?: { maxLatencyMs?: number; localOnly?: boolean };
}
interface InvokeResponse {
  outputs: Record<string, unknown>;
  implementationUsed: string;         // which Implementation was selected, per 23
  explanation: ExplanationRef;        // per 18-explainability-and-trust.md
}
```

## Algorithms

**Token verification and scope check.** Every request is verified in two steps before it reaches a
subsystem: (1) signature and expiry check against the kernel's capability monitor
([03 — Kernel Architecture](03-kernel-architecture.md#algorithms)); (2) scope match — the
requested operation (e.g. `memory:erase`) must be a subset of `token.scopes`, and if
`subjectObjectIds` is present, the request's target objects must be a subset of it. This mirrors
the kernel's own attenuation rule (a derived token is never broader than its parent) at the API
layer, so the two layers cannot drift apart.

**Context subscription diffing.** A `subscribe` call on the Context API does not re-send a full
Context Bundle on every change; the gateway keeps the last bundle sent per subscriber and pushes
only the delta (added/removed active objects, new recent Intents), computed as a set difference
against the previous snapshot — this bounds the steady-state cost of "continue yesterday's work"
staying live to the size of what actually changed, not the size of the whole bundle.

**Bounded Knowledge Graph traversal.** `KgTraverseRequest.maxDepth` is enforced server-side with a
hard ceiling independent of the caller's requested value, and traversal is executed as a
breadth-first walk with a wall-clock budget; a traversal that would exceed either bound returns a
partial result with `truncated: true` rather than blocking, consistent with
[02 §4](02-core-architecture.md#4-design-invariants)'s "degrade, never fail closed" invariant.

**Capability resolution handoff.** `InvokeRequest` never names an Implementation — only a
`contractId`. The gateway forwards the request to
[23 — Multi-Model Orchestration](23-multi-model-orchestration.md)'s router, which selects an
Implementation from the Contract's declared set (resource availability, privacy policy, latency
constraints, prior equivalence-tested behavior from [25 — SDK](25-sdk.md)'s golden suite), executes
it inside the Trust Boundary depth the [Kernel](03-kernel-architecture.md) admits it to, and returns
`implementationUsed` and an `explanation` alongside the result so the substitution is never silent.

**Memory erase cascade.** Erasing a memory record with `cascade: true` triggers a reverse-edge walk
in the Knowledge Graph to find and remove any Semantic Object relations whose sole provenance was
that memory record, then re-runs the same walk one level further to catch relations that only
existed *because* the first-level ones did — bounded by the same depth/time ceiling as
`KgTraverseRequest` — so an erase is not merely a soft "hide," but a real removal of what it
provably caused, satisfying [08 — Memory Engine](08-memory-engine.md)'s transparency and erasure
requirement end to end rather than only at the record itself.

## Interfaces / APIs

```
POST   /intent                 -> SubmitIntentResponse
GET    /intent/{id}             -> IntentSummary
DELETE /intent/{id}             -> { cancelled: boolean }

GET    /context?scope&refId     -> ContextBundleResponse
WS     /context/subscribe       -> stream<ContextBundleDelta>

POST   /memory                  -> { recordId: string }
GET    /memory/export           -> MemoryExportResponse
POST   /memory/erase            -> MemoryEraseResponse

POST   /kg/query                -> SemanticObjectRef[]
POST   /kg/traverse              -> { path: SemanticObjectRef[]; truncated: boolean }
POST   /kg/write                 -> { objectId: string }

POST   /capability/invoke        -> InvokeResponse
```

All endpoints require an `Authorization: Capability <ApiToken>` header; there is no anonymous or
ambient-authority route, matching [02 §4](02-core-architecture.md#4-design-invariants)'s "no
silent authority" invariant at the API layer, not only at the kernel layer.

## Pseudocode

```typescript
// API Gateway: the single choke point every request passes through, regardless
// of which of the five subsystems ultimately answers it.
async function handleApiRequest(req: RawRequest): Promise<RawResponse> {
  const token = parseApiToken(req.headers["Authorization"]);
  const verified = capabilityMonitor.verify(token);          // 03-kernel-architecture.md
  if (!verified.ok) return errorResponse(401, verified.reason);

  const scope = scopeFor(req.method, req.path);               // e.g. "memory:erase"
  if (!token.scopes.includes(scope)) {
    return errorResponse(403, "InsufficientScope");
  }
  if (token.subjectObjectIds && !within(req.targetObjectIds(), token.subjectObjectIds)) {
    return errorResponse(403, "OutOfScopeObject");
  }

  switch (routeFor(req.path)) {
    case "intent":      return intentEngine.handle(req);       // 05-intent-engine.md
    case "context":      return contextEngine.handle(req);      // 06/07
    case "memory":       return memoryEngine.handle(req);       // 08-memory-engine.md
    case "kg":           return knowledgeGraph.handle(req);     // 09-knowledge-graph.md
    case "capability":   return invokeCapability(req as InvokeRequest);
  }
}

async function invokeCapability(req: InvokeRequest): Promise<InvokeResponse> {
  const contract = registry.lookupContract(req.contractId);    // 24-plugin-framework.md
  const bundle = await contextEngine.fetch(req.contextBundleRef);
  const impl = await modelRouter.select(contract, {            // 23-multi-model-orchestration.md
    bundle, constraints: req.constraints,
  });
  const boundary = kernel.sandboxCreate(impl.trustDepth, impl.resourceProfile);
  try {
    const outputs = await boundary.invoke(impl, req.inputs, bundle);
    return {
      outputs,
      implementationUsed: impl.name,
      explanation: explainability.record({ contract, impl, bundle }),  // 18-*.md
    };
  } catch (fault) {
    // Degrade, never fail closed on the user's goal (02 §4 invariant 5).
    const fallback = await modelRouter.selectNextBest(contract, impl);
    if (fallback) return invokeWith(fallback, req, bundle);
    throw fault;
  }
}
```

## Security Considerations

Every API token is a capability token in the sense defined by
[03 — Kernel Architecture](03-kernel-architecture.md#data-structures): unforgeable, scoped,
expiring, and attenuable — the API layer mints no separate identity model, it re-checks the same
tokens the kernel issues, which is what keeps [02 §5](02-core-architecture.md#5-capability-security-as-the-unifying-security-model)'s
"exactly one security model" true across the whole stack rather than only below the API. Tokens are
short-lived by default and scoped to the narrowest operation and object set a caller needs;
`InvokeRequest` tokens for sensitive Contracts (per [25 — SDK](25-sdk.md)'s `permissionsRequested`)
carry `subjectObjectIds` narrowed to the specific documents in play, not blanket read/write. The
Memory API's `export` and `erase` endpoints are the concrete mechanism behind
[16 — Privacy Architecture](16-privacy-architecture.md)'s transparency guarantee and are
deliberately *not* gated behind any Capability's permission — a user's own export/erase request
against their own data always succeeds regardless of what any installed Capability declares, since
no Capability may stand between a user and their own memory. Cross-device or cross-boundary API
calls (an Agent on one device fetching Context for another) are additionally subject to
[21 — Distributed Execution](21-distributed-execution.md)'s trust model; the API layer itself does
not distinguish local from remote callers beyond the token's `boundary` field.

## Failure Modes

- **Token expiry mid-stream.** A long-lived Context subscription's token expires while the stream
  is open, requiring a silent re-authentication rather than a dropped connection the caller must
  notice and recover from manually.
- **Knowledge Graph traversal cycle or unbounded fan-out.** A malformed or adversarial query could
  otherwise consume unbounded time/memory absent the depth and wall-clock ceiling in §Algorithms.
- **Concurrent erase/read race.** A Memory `read` request in flight when an `erase` targeting the
  same record completes could observe a half-erased state.
- **No available Implementation.** A Capability Invocation request for a Contract with zero
  currently eligible Implementations (all denied by privacy policy or resource constraints).

## Recovery Mechanisms

Token expiry on an open subscription is handled by the gateway itself re-deriving a narrower,
freshly-scoped token from the caller's original grant (mirroring the kernel's `cap_derive` in
[03 — Kernel Architecture](03-kernel-architecture.md#algorithms)) and resuming the stream
transparently, so a caller only observes a renewal, never a hard failure, unless the underlying
grant itself was revoked. Traversal ceilings return a `truncated: true` partial result rather than
erroring, per [02 §4](02-core-architecture.md#4-design-invariants)'s degrade-never-fail-closed
invariant, letting the caller decide whether to page deeper. The erase/read race is closed by the
Memory API treating erase as a versioned tombstone write (consistent with
[02 §4](02-core-architecture.md#4-design-invariants)'s "everything is undoable or versioned"), so a
concurrent reader either sees the pre-erase version cleanly or the post-erase absence cleanly,
never a torn intermediate state; the tombstone itself is retained only long enough to guarantee
this and is not a way to defeat the erase. A Capability Invocation with no eligible Implementation
triggers the same fallback path [23 — Multi-Model Orchestration](23-multi-model-orchestration.md)
uses for resource exhaustion — substitute a lesser-capable Implementation and surface that
substitution via `explanation`, or, if truly none exist, return a typed `NoEligibleImplementation`
fault the calling Agent can present to the user rather than an opaque timeout.

## Performance Analysis

The gateway's token verification and scope check are designed to complete within the sub-microsecond
budget [03 — Kernel Architecture](03-kernel-architecture.md#performance-analysis) sets for
capability checks generally, since they reuse the same monitor rather than re-implementing
verification; the added cost specific to this layer is schema validation of the request body,
budgeted in the low tens of microseconds for typical payloads. Context subscription delta pushes
are bounded by the size of the change, not the bundle, per §Algorithms, keeping "continue yesterday's
work" cheap to keep live across a long session. Knowledge Graph traversal and Capability Invocation
are the two operations expected to dominate tail latency, since both may leave the local device;
their budgets are set jointly with [36 — Performance Benchmarks](36-performance-benchmarks.md) and
[37 — Scalability Roadmap](37-scalability-roadmap.md), with `constraints.maxLatencyMs` on
`InvokeRequest` as the caller's explicit lever over that trade-off.

## Trade-offs

A single gateway in front of five subsystems centralizes authentication and authorization logic —
consistent with "exactly one security model" — at the cost of a shared choke point whose own
availability every API call now depends on; this is accepted because the alternative (each
subsystem re-implementing token checks) has repeatedly been shown, across other systems, to drift
into five subtly different security models. Returning partial, truncated results for bounded
operations (Knowledge Graph traversal) rather than erroring keeps the system responsive under
adversarial or malformed input, at the cost of callers needing to explicitly check `truncated`
rather than assuming completeness; this mirrors [02 §4](02-core-architecture.md#4-design-invariants)'s
system-wide preference for graceful degradation over hard failure. Exposing `implementationUsed`
and an `explanation` on every Capability Invocation response adds a small, constant overhead to
every call in exchange for the substitution transparency [18 — Explainability & Trust](18-explainability-and-trust.md)
requires; Hyperion treats this as non-optional rather than a debug-only feature, since it is the
mechanism that keeps dynamic implementation selection from becoming a silent behavior change.

## Testing Strategy

Each of the five APIs has a contract test suite validating requests/responses against the
TypeScript-like schemas in §Data Structures, run against both a real subsystem and a
[25 — SDK](25-sdk.md)-style mock, to catch drift between the two independently of any one
Capability's tests. Authentication and authorization are fuzzed directly — malformed tokens,
expired tokens, scope-boundary edge cases, `subjectObjectIds` narrowing — as a dedicated suite
shared with [15 — Security Architecture](15-security-architecture.md)'s broader security testing.
Replay testing captures real request/response traffic from the Capability Invocation API and
re-runs it against new [23 — Multi-Model Orchestration](23-multi-model-orchestration.md) routing
logic to catch unexpected `implementationUsed` shifts before deployment. A cross-version
compatibility suite verifies that a request built against an older schema version of any of the
five APIs is either served correctly or rejected with a clear, typed version-mismatch error, never
silently misinterpreted.

---
*Next: [27 — Compatibility Layer](27-compatibility-layer.md).*
