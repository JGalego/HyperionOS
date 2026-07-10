# Networking Stack

This document defines Hyperion's **Semantic Networking Layer**: the L2 Platform Services
component (see the layer diagram in [02 — Core Architecture](02-core-architecture.md#1-layered-system-view))
that sits above a conventional, unmodified network stack and represents the internet the same
way every other subsystem represents information in Hyperion — as [Semantic
Objects](02-core-architecture.md#semantic-object) merged into the [Knowledge
Graph](09-knowledge-graph.md), not as URLs, sockets, or HTML documents. It does not replace
TCP/IP, TLS, HTTP, or DNS; it consumes them.

## 1. Purpose

Give every Hyperion [Agent](02-core-architecture.md#agent) and Capability a way to "browse" the
web that returns **people, companies, papers, products, communities, and knowledge** — the
things the [Golden Rule](01-vision-and-philosophy.md#2-the-golden-rule) says a human actually
wants — instead of a stream of raw HTML that must be re-parsed, re-summarized, and forgotten by
every Agent that touches it. A `web.research` [Capability](02-core-architecture.md#capability)
invocation resolves a query or URL into typed, citable, durable Knowledge Graph nodes and edges,
scoped and audited exactly like any other capability crossing a [Trust
Boundary](02-core-architecture.md#trust-boundary).

## 2. Motivation

In the worked trace in [02 — Core Architecture §3](02-core-architecture.md#3-how-a-request-flows-through-the-layers),
a Research Agent handling "help me prepare for tomorrow's interview" invokes `web.research` to
learn about Acme Corp. If that call returned a raw page fetch, the Agent would re-derive — every
single time, for every Agent, forever — that "Acme Corp" is an Organization, that its CEO is a
Person, that its funding announcement cites a specific Product. That work is thrown away the
moment the Agent's context window closes. Hyperion instead performs entity resolution once, at
the network boundary, and writes the result into the Knowledge Graph as durable Semantic Objects
with typed relationships. The next Agent — a different research task, a different user session,
weeks later — asks the Knowledge Graph first and only touches the network for what has changed
or is missing.

This follows directly from three commitments already made in the foundational documents:

- **Human Language First** (["01 §7"](01-vision-and-philosophy.md#7-human-language-first)): a
  user who says "find that paper about transformer efficiency" wants a paper, a Semantic Object
  with authors and a venue and citations — not a search-results page.
- **No silent authority** (["02 §4"](02-core-architecture.md#4-design-invariants)): every network
  fetch an Agent performs on the user's behalf crosses a Trust Boundary and must be
  capability-scoped, so raw socket access by Agents is architecturally disallowed, not merely
  discouraged.
- **Degrade, never fail closed** (["02 §4"](02-core-architecture.md#4-design-invariants)): when
  semantic extraction fails, the layer still returns a usable, honestly-low-confidence result
  rather than blocking the Agent's goal.

## 3. Architecture

The Semantic Networking Layer is a service within L2 Platform Services. It never replaces the
conventional transport stack underneath it; it is a consumer of that stack, exactly as a
traditional browser is.

```
 L4  Cognition Layer        Research Agent, Coding Agent, ...
                                  │ invokes Capability `web.research(query | url, purpose)`
                                  ▼
 L3  Knowledge Layer         Knowledge Graph (09) ◄──── merge ──── Web Entity Resolver
                                  ▲                                       │
                                  │ resolved Semantic Objects              │ candidate entities
                                  │ (Paper, Person, Org, Product, ...)      │
 ────────────────────────────────┴───────────────────────────────────────┘
 L2  Platform Services       ┌─────────────────────────────────────────────┐
      Semantic Networking    │  Capability Gateway  (web.research,          │
      Layer                  │    web.fetch.raw for 27-compat only)         │
                              │  Web Entity Resolver Pipeline                │
                              │    fetch → extract → classify → link → merge │
                              │  Resolution Cache / Dedup Index              │
                              │  Provenance Ledger (audit trail)             │
                              └───────────────────┬───────────────────────┘
                                                   │ conventional requests
                                                   ▼
                              HTTP/1.1, HTTP/2, HTTP/3(QUIC), TLS 1.3, DNS
 ────────────────────────────────────────────────────────────────────────
 L1  System Runtime          Sandboxed network egress process (per-Agent)
 L0  Kernel                  NIC driver, socket primitives, packet filter
```

Three properties keep this architecture from becoming "a browser with extra steps":

1. **Agents never hold a socket.** An Agent's only network-shaped verb is a Capability
   (`web.research`, or `web.fetch.raw` for the [Compatibility
   Layer](27-compatibility-layer.md)). The conventional stack is reachable only from inside the
   Capability Gateway's sandboxed egress process, itself a
   [Trust Boundary](02-core-architecture.md#trust-boundary) enforced by the [Kernel's driver
   model](03-kernel-architecture.md) and scheduled like any other workload by the
   [Scheduler](04-scheduler.md).
2. **Resolution happens once, at the boundary, not once per Agent.** The Web Entity Resolver is
   the single place raw bytes are turned into meaning; everything above L2 only ever sees
   Semantic Objects.
3. **The compatibility path is a parallel, explicitly lower-trust lane**, not a fallback inside
   the semantic path — see §3.1.

### 3.1 Relationship to the Compatibility Layer

[27 — Compatibility Layer](27-compatibility-layer.md) hosts legacy web applications (a bank's
web portal, a SPA that expects real cookies, a canvas-heavy web game) that require actual browser
semantics — DOM, JavaScript execution, session cookies, rendered pixels — not entity graphs. These
requests are issued through `web.fetch.raw`, a distinct Capability with its own contract, trust
tier, and audit category. `web.fetch.raw` responses are **never** fed into the Web Entity
Resolver or merged into the Knowledge Graph by default; a user or Agent may explicitly request
"also extract what you can from this page," which routes the same bytes through the resolver
pipeline as a secondary, opt-in step. The two Capabilities share the same underlying conventional
network stack and the same capability-scoping and audit machinery in
[15 — Security Architecture](15-security-architecture.md); they differ only above the transport
line.

## 4. Data Structures

| Structure | Fields | Purpose |
|---|---|---|
| `WebResolutionRequest` | `request_id`, `origin` (query text or URL), `intent_id`, `agent_id`, `capability_token`, `purpose`, `freshness_policy`, `depth` | Carries the scoped intent behind a fetch; `purpose` and `depth` bound how much of the web the resolver is allowed to traverse (e.g. "one page" vs "follow citations two hops"). |
| `CanonicalURL` | `raw_url`, `canonical_form`, `redirect_chain[]`, `domain`, `robots_txt_state` | Produced by normalization (§5.1); the unit of cache and dedup keying. |
| `ResolvedEntityCandidate` | `content_hash`, `entity_type` (Paper, Person, Organization, Product, Community, Concept, Event, Place, WebPage-fallback), `extracted_fields{}`, `confidence`, `source_url`, `retrieved_at` | The resolver's proposed Semantic Object before merge. |
| `ProvenanceRecord` | `object_id`, `source_url`, `resolver_version`, `extraction_method` (structured-data / model-based), `confidence`, `timestamp` | Attached to every merged Semantic Object and edge; the audit trail required by [18 — Explainability & Trust](18-explainability-and-trust.md). |
| `ResolutionCacheEntry` | `canonical_url_hash`, `object_id`, `content_fingerprint`, `last_verified`, `ttl_class`, `ttl_expiry` | Backs dedup (§5.2); `ttl_class` differs by entity type (a paper's metadata is near-immutable; a product's price is not). |
| `DomainEgressGrant` | `capability_token_id`, `domain_pattern[]`, `rate_limit`, `max_depth`, `expiry` | The scoping data attached to a `web.research` capability grant; enforced at the Capability Gateway, not trusted to the Agent. |

## 5. Algorithms

### 5.1 URL Canonicalization

Strip tracking parameters, resolve redirect chains fully, prefer `<link rel="canonical">` /
`schema.org` identifiers over the requested URL, and normalize scheme/host casing. Canonicalization
happens before any cache lookup so that `https://x.com/a?utm_source=…` and `https://x.com/a`
dedupe to the same entity.

### 5.2 Dedup / Cache Lookup

Hash the `CanonicalURL`; check the Resolution Cache. On a hit within `ttl_expiry`, return the
cached `object_id` directly with no network egress. On a hit past `ttl_expiry`, issue a
conditional revalidation request (`If-None-Match` / `If-Modified-Since` where the origin supports
it) rather than a full re-fetch.

### 5.3 Extraction and Classification

Structured signals are preferred over model inference, in this order: `schema.org`/JSON-LD,
OpenGraph metadata, identifier microformats (DOI, ORCID, ISBN, GTIN), then — only if none of the
above resolves an entity type with sufficient confidence — a local model pass (routed through
[22 — Local AI Runtime](22-local-ai-runtime.md), escalating to a routed cloud model via
[23 — Multi-Model Orchestration](23-multi-model-orchestration.md) only under the same
local-first consent rules as every other Capability) extracts entity type and fields from
unstructured page text.

### 5.4 Entity Resolution Against the Knowledge Graph

Match the candidate against existing nodes by, in priority order: (a) exact external identifier
(DOI, ORCID, company registration ID), (b) high-confidence embedding similarity above a
configured merge threshold, (c) no match → create a new node. Ambiguous cases below the merge
threshold but above a "clearly distinct" floor are not silently merged — see §9 and §10.

### 5.5 Relationship Extraction

Typed edges (`authored_by`, `cites`, `employs`, `produces`, `member_of`) are extracted alongside
the entity itself and written into the Knowledge Graph in the same transaction as the node, with
the same `ProvenanceRecord` attached to the edge.

## 6. Interfaces / APIs

| Capability | Direction | Contract |
|---|---|---|
| `web.research(query \| url, purpose, freshness, depth)` | Agent → Semantic Networking Layer | Returns `SemanticObjectRef[]` + a natural-language summary; side effect: Knowledge Graph writes; permission required: scoped domain egress grant; every call is audited (see [15 — Security Architecture](15-security-architecture.md)). |
| `web.fetch.raw(url, render_mode)` | Agent/Compatibility Layer → Semantic Networking Layer | Returns raw bytes/DOM snapshot for [27 — Compatibility Layer](27-compatibility-layer.md); no Knowledge Graph merge unless explicitly chained into `web.research.extract_from(bytes)`. |
| `EntityResolver.resolve(url_or_query, hints) → ResolvedEntitySet` | Internal | Used by `web.research`; not directly callable by Agents. |
| `KnowledgeGraph.mergeCandidate(candidate) → ObjectID` | Internal | Defined authoritatively in [09 — Knowledge Graph](09-knowledge-graph.md); the resolver is one of several writers. |
| Event `web.entity.resolved` | Published on the [Event System](31-event-system.md) | Lets other Agents/Workspaces react to newly resolved entities without polling. |

## 7. Pseudocode

```python
def web_research(request: WebResolutionRequest) -> list[SemanticObjectRef]:
    grant = capability_gateway.authorize(request.capability_token, request.origin)
    if not grant.permits(request.origin):
        audit.log_denied(request)
        raise CapabilityDenied(f"{request.agent_id} not granted egress to {request.origin}")

    canonical = canonicalize(request.origin, follow_redirects=True)
    cache_hit = resolution_cache.lookup(canonical)

    if cache_hit and not cache_hit.expired():
        audit.log_cache_hit(request, cache_hit.object_id)
        return [SemanticObjectRef(cache_hit.object_id)]

    try:
        raw = fetch_via_conventional_stack(canonical, grant)   # HTTP/TLS, sandboxed egress
    except (DNSError, TLSError, TimeoutError) as e:
        return degrade_to_stale_or_stub(canonical, cache_hit, error=e)   # §10

    sanitized = sandbox_parse(raw)                # untrusted content, never executed as code
    quarantined = injection_scanner.scan(sanitized)  # treat page content as data, not instructions
    if quarantined.suspicious:
        audit.log_quarantine(request, quarantined.reason)
        return [stub_object(canonical, note="content withheld pending review")]

    candidates = extract_entities(sanitized, hints=request.purpose)   # §5.3
    resolved = []
    for candidate in candidates:
        match = knowledge_graph.find_match(candidate)              # §5.4
        if match.confidence >= MERGE_THRESHOLD:
            object_id = knowledge_graph.merge(match.object_id, candidate)
        elif match.confidence <= DISTINCT_FLOOR:
            object_id = knowledge_graph.create(candidate)
        else:
            object_id = knowledge_graph.create_provisional(candidate, needs_review=True)
        knowledge_graph.attach_provenance(object_id, canonical, candidate)
        resolution_cache.write(canonical, object_id, ttl=ttl_for(candidate.entity_type))
        events.publish("web.entity.resolved", object_id)
        resolved.append(SemanticObjectRef(object_id))

    return resolved
```

## 8. Security Considerations

- **Capability scoping, not URL blocklisting.** Every `web.research` grant carries a
  `DomainEgressGrant` (domain patterns, rate limit, max traversal depth, expiry). The Agent
  never sees or controls raw egress; the Capability Gateway enforces the grant, mirroring the
  single capability-security model described in [02 §5](02-core-architecture.md#5-capability-security-as-the-unifying-security-model).
- **SSRF containment.** The egress sandbox refuses requests to private/link-local address
  ranges and to the local device's own service ports regardless of grant contents; this is a
  Kernel-enforced Trust Boundary rule per [03 — Kernel Architecture](03-kernel-architecture.md),
  not an application-level check an Agent could bypass.
- **TLS is non-negotiable.** Certificate validation failures are always a hard failure surfaced
  to §9, never silently downgraded to plaintext.
- **Content is data, never instructions.** Fetched pages are prompt-injection surface: page text
  can contain "ignore your instructions and…" strings targeted at the Agent's underlying model.
  The pipeline sandbox-parses content and routes it through an injection scanner (see the
  quarantine step in §7) before it ever reaches a model context; the extraction model in §5.3 is
  invoked with the page as clearly-delimited untrusted data, per the threat categories in
  [17 — Threat Model](17-threat-model.md).
- **Cache poisoning resistance.** Cache hits past `ttl_expiry` are revalidated, not blindly
  served; `content_fingerprint` mismatches invalidate the entry rather than silently updating it
  without re-running the extraction and merge pipeline.
- **Privacy leakage via egress.** Query text and referrer data sent upstream are minimized by
  default (no forwarding of unrelated Context Bundle contents) per
  [16 — Privacy Architecture](16-privacy-architecture.md); a research query about a medical
  condition does not leak the user's identity or unrelated Semantic Objects to the origin server.
- **Full audit trail.** Every fetch, merge, cache hit, and quarantine event is logged with the
  requesting `agent_id` and `intent_id`, satisfying the auditability invariant in
  [01 §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable).

## 9. Failure Modes

| Failure | Detection | Immediate effect |
|---|---|---|
| DNS resolution failure / TLS handshake failure | Transport error from conventional stack | No candidate produced; §10 stale/stub path taken |
| Timeout / origin unreachable | Deadline exceeded | Same as above |
| `robots.txt` disallow or 429 rate-limited | Gateway pre-check / origin response | Request skipped, logged, optionally retried later |
| Paywall / auth required | Non-2xx or partial content | Low-confidence stub Semantic Object with a note, not a hard failure |
| Extraction produces no confident entity | Extraction confidence below floor | Falls back to a generic `WebPage` Semantic Object with title/summary only |
| Ambiguous entity match (near-threshold) | §5.4 scoring | Provisional node created, flagged `needs_review` |
| Stale cache serving outdated fact | TTL expiry / fingerprint mismatch | Revalidation triggered on next access |
| Capability token expires mid-task | Gateway authorization check | Remaining requests denied; Agent notified to re-request the grant |
| Suspected prompt injection in fetched content | Injection scanner | Content quarantined, withheld from Knowledge Graph merge |

## 10. Recovery Mechanisms

Consistent with the "degrade, never fail closed" invariant in
[02 §4](02-core-architecture.md#4-design-invariants):

- **Stale-but-labeled fallback**: if a live fetch fails but a cached (even expired) entity
  exists, return it with an explicit `stale: true` marker rather than nothing — an Agent can
  decide whether staleness matters for its purpose.
- **Human-in-the-loop disambiguation**: provisional/`needs_review` merges from §9 surface to the
  user through the active Workspace ("I found two possible matches for 'John Smith' — same
  person?"), consistent with [18 — Explainability & Trust](18-explainability-and-trust.md); the
  Knowledge Graph never silently commits an ambiguous merge.
- **Per-domain circuit breaker**: repeated failures against one domain trip a breaker that fails
  fast for a cooldown window instead of retrying every Agent request against a down origin.
- **Quarantine-and-review, not discard**: content flagged by the injection scanner is retained
  in a review queue rather than deleted, so a security reviewer can confirm true/false positives
  and tune the scanner (feeding [35 — Testing Strategy](35-testing-strategy.md)).
- **Re-grant flow**: an expired capability token triggers a re-authorization prompt scoped to
  the same Intent rather than silently failing the Agent's whole task.

## 11. Performance Analysis

Cache hit rate dominates end-to-end latency: a resolved, unexpired entity returns in
single-digit milliseconds (local Knowledge Graph lookup only), while a cold resolution pays for
DNS + TLS + HTTP round trip (typically 100–800ms depending on origin and geography) plus
extraction. Structured-data extraction (§5.3, JSON-LD/OpenGraph path) completes in well under
200ms on-device; unstructured-page model extraction is the dominant cost (hundreds of
milliseconds to low seconds depending on the local model tier selected by
[22 — Local AI Runtime](22-local-ai-runtime.md)) and is the primary reason caching and dedup
exist at all. A Research Agent evaluating twenty search results issues bounded-parallel fetches,
capped by the resource profile the [Scheduler](04-scheduler.md) assigns to the invoking
Capability — this prevents one Agent's research task from starving other network-bound work.
Cache sizing is dominated by entity count, not raw page bytes, since raw HTML is discarded after
extraction; only `ResolvedEntityCandidate` fields and a content fingerprint are retained.
Concrete latency and throughput targets are tracked in
[36 — Performance Benchmarks](36-performance-benchmarks.md).

## 12. Trade-offs

- **Semantic merge cost vs. raw browsing speed.** Resolving entities is strictly more expensive
  per-fetch than returning bytes. This is accepted because the cost is paid once per entity, not
  once per Agent-request, and amortizes rapidly across the system's lifetime — the opposite of a
  traditional browser's per-tab, throwaway parse.
- **Single shared Knowledge Graph truth vs. per-Agent scratch copies.** A shared, merged
  representation means every Agent benefits from prior resolution work, at the cost of needing
  careful merge-collision handling (§5.4, §10) so that one Agent's sloppy extraction doesn't
  corrupt another's trusted data.
- **Freshness vs. dedup.** Aggressive caching risks staleness for time-sensitive entities (a
  product's price, a company's headcount); this is managed via per-entity-type TTL classes
  (§4) rather than a single global cache lifetime.
- **Local-first extraction vs. accuracy.** Preferring local models for extraction protects
  privacy and cost (per [22 — Local AI Runtime](22-local-ai-runtime.md)) but may be less
  accurate on obscure unstructured pages than a larger routed cloud model; the Model Router in
  [23 — Multi-Model Orchestration](23-multi-model-orchestration.md) makes this escalation
  decision under the same consent rules as every other Capability.
- **Two capability lanes (`web.research` vs `web.fetch.raw`) is added complexity** but is
  necessary: collapsing them would either force the semantic layer to fully emulate a browser
  (defeating its purpose) or force legacy web apps in
  [27 — Compatibility Layer](27-compatibility-layer.md) through an entity-resolution pipeline
  that actively breaks their expected raw-DOM/cookie semantics.

## 13. Testing Strategy

- **Golden extraction corpus**: a curated set of real pages with known correct entities
  (papers with known DOIs, companies with known registration IDs) used to regression-test
  extraction precision/recall on every resolver change.
- **Canonicalization unit tests**: tracking-parameter stripping, redirect-chain resolution, and
  identifier-preference ordering against adversarial URL variants.
- **Merge-collision tests**: near-duplicate entities (two distinct people with the same name and
  overlapping fields) verified to land in `needs_review`, not silently merged.
- **Capability-scoping security tests**: attempted SSRF targets, disallowed domains, and expired
  tokens verified to be denied and audited, per [15 — Security Architecture](15-security-architecture.md).
- **Prompt-injection red-team corpus**: pages containing known injection patterns verified to be
  quarantined and never reach a model context unsanitized.
- **Chaos tests**: simulated DNS failure, TLS failure, and timeout injected at the transport
  boundary, verifying §10's stale-fallback and circuit-breaker behavior actually triggers.
- **Cache correctness tests**: TTL expiry, fingerprint-mismatch invalidation, and revalidation
  round trips.
- **Performance regression gates** tied to the budgets in
  [36 — Performance Benchmarks](36-performance-benchmarks.md).

---
*Next: [20 — Device Framework](20-device-framework.md).*
