# Knowledge Graph

## 1. Purpose

The Knowledge Graph is the L3 [Knowledge Layer](02-core-architecture.md#1-layered-system-view)
subsystem that stores every [Semantic Object](02-core-architecture.md#semantic-object) in
Hyperion — the Universal Object System — as a typed node in a single evolving graph of typed,
weighted relationships, and makes that graph queryable by meaning rather than by name. It is the
backing store for [10 — Semantic Filesystem](10-semantic-filesystem.md)'s folder/file
compatibility view and for [06 — Context Engine](06-context-engine.md)'s Context Bundle assembly,
and it holds the durable tiers of the [Memory Engine](08-memory-engine.md): episodic, semantic,
procedural, and long-term records are themselves Semantic Objects that live in this graph.

## 2. Motivation

[01 — Vision & Philosophy](01-vision-and-philosophy.md) states the unit of stored information is
a Semantic Object, "not a file in a folder" — replacing name-based, hierarchical organization with
meaning-based organization is one of the three technology curves (§3) that make Hyperion possible
at all. The brief's illustrative query — "Find the paper about quantum computing I read six
months ago" — is deliberately unanswerable by a filesystem: there is no filename, no folder, no
exact date. It is answerable by a graph that knows the object is a research paper, has an
embedding close to "quantum computing," and has a `read-by` edge from the user with a timestamp
near six months prior. Making that query work is the central engineering problem this document
solves: reasoning about what the user wants becomes the same operation as searching the graph.

## 3. Architecture

```
                     ┌─────────────────────────────────────────────┐
                     │        Hybrid Query Engine (§5, §7)           │
                     │  graph pattern ∩ vector similarity ∩ time     │
                     │  ∩ permission filter → ranked results          │
                     └───────┬─────────────────┬───────────────┬────┘
                             │                 │               │
                    ┌────────▼─────┐  ┌────────▼──────┐  ┌─────▼──────┐
                    │ Graph Store   │  │ Vector Index  │  │ Temporal   │
                    │ (nodes+edges, │  │ (ANN / HNSW,  │  │ Index      │
                    │  adjacency)   │  │  per node type)│  │ (time-range)│
                    └────────┬─────┘  └────────┬──────┘  └─────┬──────┘
                             │                 │                │
                    ┌────────▼─────────────────▼────────────────▼────┐
                    │              NODES = Semantic Objects            │
                    │  Document · Photo · Video · Audio · Message ·    │
                    │  Person · Meeting · Project · Task · Company ·   │
                    │  Code · Repository · ResearchPaper · Device ·    │
                    │  Knowledge (concept)                              │
                    │                                                    │
                    │  EDGES = typed, weighted, directed                  │
                    │  explicit: authored-by · part-of · attends ·       │
                    │            owns · replies-to · references          │
                    │  inferred: semantically-similar-to · discussed-in  │
                    │            · derived-from · co-occurs-with          │
                    └───────────────────────┬────────────────────────────┘
                                            │ capability-checked at every hop
                     ┌──────────────────────┼──────────────────────┐
                     ▼                      ▼                      ▼
          [10 Semantic Filesystem]  [06 Context Engine]   [08 Memory Engine]
          folder/path view is a     Context Bundle =      episodic/semantic/
          traversal, not storage    subgraph around        procedural records
                                    active Intent          stored as nodes here
```

Writes arrive concurrently from multiple [Agents](02-core-architecture.md#agent), each executing
inside its own [Trust Boundary](02-core-architecture.md#trust-boundary); the graph store's
consistency algorithm (§5.4) is what keeps the shared graph coherent under that concurrency
without requiring a single global lock.

## 4. Data Structures

The concrete on-disk schema for the tables below lives in
[29 — Database Schema](29-database-schema.md); this section defines the logical model.

```
Node (Semantic Object) {
  id:                  ObjectID
  type:                enum{Document,Photo,Video,Audio,Message,Person,Meeting,
                             Project,Task,Company,Code,Repository,ResearchPaper,
                             Device,Concept, ...}       // extensible per 24-plugin-framework.md
  content_ref:          BlobRef              // raw bytes live in 28-storage-engine.md
  embedding:            vector<f32>[d]        // multi-vector for multi-modal objects
  metadata:             {title?, created_at, modified_at, source, mime_type, ...}
  permissions:          ACL / capability scope (15-security-architecture.md)
  version_history:      [VersionRef]          // append-only
  reasoning_provenance: [{agent_id, intent_id, action, timestamp}]
}

Edge {
  id:               EdgeID
  from, to:          ObjectID
  type:              ExplicitType | InferredType
  weight:            float [0,1]              // confidence / strength
  directed:          bool
  provenance:        {kind: EXPLICIT | INFERRED, agent_id?, model_version?, timestamp}
  version_vector:     map<replica_id, counter> // for CRDT merge under concurrent writes
  tombstone:         bool                      // soft-deleted, never resurrected
}
```

Index structures: the **graph store** holds adjacency lists keyed by node and edge type; the
**vector index** is an ANN structure (HNSW-class) partitioned by node type for fast type-filtered
similarity search; the **temporal index** is a range index over `created_at`/event timestamps
enabling fuzzy "around six months ago" windows (§7); a full-text fallback index exists for
exact-string/compatibility queries from [27 — Compatibility Layer](27-compatibility-layer.md).

## 5. Algorithms

### 5.1 Embedding generation

On every Semantic Object create or modify, a local model
([22 — Local AI Runtime](22-local-ai-runtime.md)) extracts content (text, transcript, visual
features, code AST) and computes an embedding, written to the node and queued for the vector index
asynchronously, bounded by a staleness SLA (target < 5s, §11).

### 5.2 Inferred edge computation

A background job (scheduled low-priority per [04 — Scheduler](04-scheduler.md), mirroring the
[Memory Engine](08-memory-engine.md)'s consolidation cycle) computes:

- **semantically-similar-to**: k-NN in embedding space above similarity threshold τ_sim, creating
  or refreshing an edge with `weight = cosine_similarity`.
- **discussed-in / co-occurs-with**: objects referenced or opened together within the same
  Episodic Memory record ([08 — Memory Engine](08-memory-engine.md)) accrue an edge whose weight
  is the normalized co-occurrence frequency.

Inferred edges that are never re-confirmed by continued co-occurrence or continued similarity
decay in weight over time using the same recency-weighted mechanism as
[08 — Memory Engine §5.2](08-memory-engine.md#52-decay-weighted-consolidation-not-ttl) — an
inferred edge, unlike an explicit one, is a hypothesis and is allowed to fade.

### 5.3 Reasoning-becomes-search query translation

A natural-language recall request is translated by the
[Intent Engine](05-intent-engine.md) into a structured `GraphQuery`, executed by the Hybrid Query
Engine as: type/relationship filter ∩ vector top-k ∩ temporal window ∩ permission filter, re-ranked
by a combined score. Worked example in §7.

### 5.4 Concurrency control under multi-agent writes

Node metadata uses **optimistic concurrency**: every write carries the version it read; a
conflicting concurrent write is rejected and retried against the new version (compare-and-swap),
which is cheap because node metadata writes are small and contention on any one node is rare. Edge
sets use a **CRDT (grow-with-tombstones) merge**: concurrent edge insertions from different
Agents/devices are unioned rather than conflicting, and edge deletions are tombstones carrying a
version vector so a deletion is never silently undone by a late-arriving insertion from a device
that hadn't seen it yet. Multi-edge operations that must be atomic — e.g., moving a Task between
two Projects, which requires removing one `part-of` edge and adding another as a unit — run as a
local transaction within a single Trust Boundary; cross-device atomic operations use a saga with
compensating actions, coordinated via [30 — IPC Framework](30-ipc-framework.md) messaging, rather
than a distributed lock, consistent with the offline-first, availability-favoring stance of
[21 — Distributed Execution](21-distributed-execution.md).

## 6. Interfaces / APIs

```
graph.get(id)                                  -> Node
graph.query(GraphQuery{type, pattern, embedding_query,
                        time_range, edge_constraints, limit}) -> [Node]  (ranked, §7)
graph.traverse(start_id, edge_types, depth)     -> Subgraph
graph.link(from, to, type, weight?)             -> Edge     // capability-checked
graph.unlink(edge_id)                           -> Tombstone
graph.subscribe(pattern)                        -> EventStream           // 31-event-system.md
graph.explain(node_id | edge_id)                -> ProvenanceChain        // 18-explainability-and-trust.md
```

[10 — Semantic Filesystem](10-semantic-filesystem.md) renders a folder/path view purely by
traversing `part-of`/`contains` edges on demand — no separate storage of "where a file lives."
[06 — Context Engine](06-context-engine.md) assembles a Context Bundle by calling `graph.traverse`
outward from the active Intent's anchor objects up to a relevance-bounded depth.

## 7. Pseudocode

Worked example for the brief's canonical query, "Find the paper about quantum computing I read
six months ago":

```python
def resolve_recall_query(utterance, principal, now):
    # 1. Intent Engine (05) parses slots from the utterance
    slots = intent_engine.parse(utterance)
    # slots = {object_type: "ResearchPaper", topic: "quantum computing",
    #          time_hint: "six months ago", relation_hint: "read-by-user"}

    # 2. Build a fuzzy temporal window around the hint rather than an exact date
    center = now - months(6)
    window = TemporalWindow(mean=center, sigma=days(30))   # Gaussian prior, not exact match

    query = GraphQuery(
        type_filter=[slots.object_type, "Document"],
        embedding_query=embed(slots.topic),                 # "quantum computing"
        time_range=window,
        edge_constraints=[EdgeConstraint(type="read-by", target=principal)],
        limit=10,
    )

    # 3. Execution: vector ANN restricted to type + coarse time pre-filter
    candidates = vector_index.search(query.embedding_query,
                                      type_filter=query.type_filter,
                                      time_prefilter=window.coarse_bounds())

    # 4. Verify the relationship constraint via graph traversal
    #    (explicit read-by edge, or an episodic VIEWED event from 08-memory-engine.md)
    candidates = [c for c in candidates
                  if graph.has_edge(principal, c.id, type="read-by")
                  or episodic_memory.has_view_event(principal, c.id)]

    # 5. Re-rank: similarity, edge confidence, temporal proximity to the fuzzy center
    ranked = sorted(candidates, key=lambda c: (
        0.5 * c.similarity +
        0.3 * c.read_by_edge_weight +
        0.2 * window.density(c.metadata.viewed_at)
    ), reverse=True)

    # 6. Enforce permission filter last, defense in depth
    ranked = [c for c in ranked if capability_check(principal, c.id, "read")]

    return with_provenance(ranked[:query.limit])   # 18-explainability-and-trust.md
```

The result is explainable: for each returned object, `graph.explain` can show the cosine
similarity to "quantum computing," the `read-by` edge and when it was created, and why it fell
inside the fuzzy six-month window — turning "search" into visible reasoning, per
[18 — Explainability & Trust](18-explainability-and-trust.md).

## 8. Security Considerations

Every hop of every traversal is capability-checked, not just the query root: an Agent permitted to
read Node A must not silently reach Node B merely because an edge connects them — this is
[02 — Core Architecture](02-core-architecture.md#4-design-invariants) invariant #1, "no silent
authority," applied to graph traversal specifically. Inferred edges are never used as a basis for
access decisions — only explicit, provenance-checked edges (e.g., `owns`, `shared-with`) grant
reachability across a permission boundary; a `semantically-similar-to` edge can surface a result
for discovery but can never leak an object the requester lacks capability for, because the
permission filter is applied as the final stage of every query (§7, step 6) regardless of how a
candidate was found. Cross-device graph sync encrypts deltas in transit and at rest
([19 — Networking Stack](19-networking-stack.md), [28 — Storage Engine](28-storage-engine.md)). A
malicious Capability attempting to poison the graph — e.g., writing a false `authored-by` edge to
misattribute ownership — is constrained by requiring every explicit edge write to carry
attributable provenance and by anomaly detection over edge-write patterns (see
[17 — Threat Model](17-threat-model.md)).

## 9. Failure Modes

- **Conflicting concurrent metadata writes** to the same node — handled by CAS retry (§5.4);
  repeated conflict beyond a retry budget surfaces to the user rather than silently picking a
  winner.
- **Embedding index staleness** — a newly-created object exists in the graph but isn't yet
  semantically searchable within the SLA window; graph/keyword search still finds it meanwhile.
- **False-positive inferred edges** ("hallucinated" similarity) — bounded by the similarity
  threshold and by decay of unconfirmed inferred edges (§5.2).
- **Orphaned nodes** with no incoming edges reduce discoverability — a periodic connectivity job
  surfaces them via recency-based queries as a fallback.
- **Partition during offline device use** — the local subgraph continues operating; on reconnect,
  CRDT edge merge and node version vectors reconcile automatically, with unresolved metadata
  conflicts surfaced to the user (§5.4).
- **Store corruption** — recoverable from the append-only version/edge log plus
  [28 — Storage Engine](28-storage-engine.md) snapshots.

## 10. Recovery Mechanisms

Every node carries its own `version_history`, so any bad write — including one caused by a
compromised or buggy Capability — is revertible via
[33 — Rollback & Recovery](33-rollback-recovery.md). Edge deletions are tombstones, not physical
removals, so an accidental `unlink` is undoable within a retention window. A background
consistency checker scans for dangling edges (pointing at purged nodes), duplicate inferred
edges, and stale embeddings, repairing automatically where safe and flagging ambiguous cases for
user review. Upgrading the embedding model in
[22 — Local AI Runtime](22-local-ai-runtime.md) triggers a full re-embedding backfill job that
recomputes vectors without touching graph structure. The mutation audit log
([34 — Observability & Telemetry](34-observability-telemetry.md)) allows point-in-time graph
reconstruction.

## 11. Performance Analysis

Hybrid queries (§7) are budgeted against [06 — Context Engine](06-context-engine.md)'s
context-assembly latency and the conversational responsiveness targets in
[36 — Performance Benchmarks](36-performance-benchmarks.md): target sub-150ms on-device for a
type-and-time-prefiltered ANN search plus edge verification at up to ~10M objects. Node/explicit-
edge writes are fast (target <10ms) because they don't wait on embedding computation, which
trails asynchronously. Inferred-edge computation is batched and incremental (approximate k-NN via
the ANN index rather than pairwise O(n²) comparison) to keep the background job's cost sublinear
in graph size. Scaling beyond a single device — sharding by owner/workspace and federated
cross-shard queries — is addressed in
[37 — Scalability Roadmap](37-scalability-roadmap.md) and
[21 — Distributed Execution](21-distributed-execution.md).

## 12. Trade-offs

Storing an embedding for every Semantic Object (storage and compute cost) is accepted because
universal semantic search is the core thesis — "everything connects" — with cost controlled via
quantized embeddings and lazy computation for rarely-touched objects. Eventual, CRDT-based
consistency for edges over a globally strongly-consistent graph is chosen for offline-first
availability across devices ([21 — Distributed Execution](21-distributed-execution.md)), accepting
brief windows where inferred edges reflect slightly stale state; explicit metadata writes still
get CAS correctness where ordering matters. Confidence-weighted, epistemically-labeled inferred
edges — versus treating every edge as equally certain — add ranking complexity but let the UI
present discovery results with appropriate "I think" framing rather than false certainty
([18 — Explainability & Trust](18-explainability-and-trust.md)). A single unified graph across all
object types, versus per-domain siloed stores, is chosen because siloing would directly contradict
"everything connects," at the cost of needing one permission model expressive enough to span every
domain ([15 — Security Architecture](15-security-architecture.md)).

## 13. Testing Strategy

Schema and property tests validate every node/edge type's invariants (e.g., tombstoned edges never
reappear). Concurrency stress tests run N simulated Agents writing conflicting edges and metadata
to the same nodes and assert CRDT convergence with no lost explicit edit. Query-correctness tests
run a labeled synthetic corpus of "find the X about Y from around Z time ago" queries and tune the
fuzzy-window parameters (§7) against expected top-k recall. Adversarial tests attempt traversal
into unauthorized objects via inferred edges and confirm the permission filter blocks them
regardless of edge type ([17 — Threat Model](17-threat-model.md)). Chaos tests kill the
inferred-edge background job mid-batch and verify resumability without duplicate or corrupt
edges. A regression suite re-runs after every embedding-model upgrade to confirm re-embedding
preserves ranking quality, feeding into the general framework in
[35 — Testing Strategy](35-testing-strategy.md). Scale tests validate the latency budgets in §11
at target object counts, feeding [36 — Performance Benchmarks](36-performance-benchmarks.md) and
[37 — Scalability Roadmap](37-scalability-roadmap.md).

---
*Next: [10 — Semantic Filesystem](10-semantic-filesystem.md).*
