# Database Schema

## Purpose

This document gives the concrete logical schema materialized inside the metadata store, graph
index, and vector index of the [28 — Storage Engine](28-storage-engine.md): the actual tables,
columns, types, and indexes that every Semantic Object, relationship, and version in Hyperion is
made of. Where [28](28-storage-engine.md) describes the engine that makes writes to this schema
atomic, encrypted, and replicated, this document describes the shape of the data itself, so that
[09 — Knowledge Graph](09-knowledge-graph.md) and [10 — Semantic Filesystem](10-semantic-filesystem.md)
can be read as two different *query shapes* over one physical schema rather than two different
storage systems.

## Motivation

A schema this central has to satisfy contradictory-looking requirements at once: it must support
graph traversal (multi-hop, variable predicate) for the Knowledge Graph, similarity search for
semantic recall, POSIX-shaped directory listings for legacy compatibility, fine-grained
per-object ACLs for [15 — Security Architecture](15-security-architecture.md), and full version
history for [33 — Rollback & Recovery](33-rollback-recovery.md) — without maintaining five
separate copies of the same fact. The design here resolves that by keeping exactly three tables
of record (`semantic_objects`, `edges`, `object_versions`) plus one thin convenience table
(`collections`) that is itself expressed in terms of the first two, and letting every higher-level
document query those same tables differently rather than duplicating them.

## Architecture

```
 semantic_objects (one row per Semantic Object, current-version snapshot)
 ┌───────────────────────────────────────────────────────────────────┐
 │ object_id PK  object_type  embedding  metadata JSONB  acl JSONB   │
 │ blob_hash  version_id FK   owner_id   device_origin   timestamps  │
 └───────────────────────────────────────────────────────────────────┘
        │ 1                                          │ 1
        │ subject_id / object_id (many)              │ version_id (many)
        ▼                                            ▼
 edges                                       object_versions
 ┌───────────────────────────────────┐       ┌─────────────────────────────┐
 │ edge_id PK  subject_id FK         │       │ version_id PK  object_id FK │
 │ predicate   object_id FK          │       │ parent_version FK (self)    │
 │ weight  provenance  origin        │       │ blob_hash  metadata_diff    │
 │ confidence  expires_at  owner_id  │       │ actor  wal_offset  hlc_ts   │
 └───────────────────────────────────┘       └─────────────────────────────┘
        ▲
        │ predicate = 'member_of', origin = 'explicit'
 collections  (a semantic_objects row of type='collection' + membership edges)
 ┌───────────────────────────────────────────────────┐
 │ collection_id PK/FK → semantic_objects.object_id  │
 │ display_name  is_user_created  sort_order         │
 └───────────────────────────────────────────────────┘

   Knowledge Graph (09) reads: recursive CTE over `edges`, ANN query over `semantic_objects.embedding`
   Semantic Filesystem (10) reads: same two queries, re-shaped into directory listings
```

Only two tables carry facts about the world (`semantic_objects`, `edges`); `object_versions` is
derived history and `collections` is a thin view over the other two. This is the concrete
realization of the "materialized views over one log" principle from
[28 — Storage Engine](28-storage-engine.md#architecture): nothing here is written outside of an
Object Write Transaction.

## Data Structures

**`semantic_objects`** is the current-version snapshot of every Semantic Object: identity,
type, embedding, free-form metadata, ACL, and a pointer to its version chain. **`edges`** is the
relationship layer the Knowledge Graph reasons over — deliberately one table for every predicate
rather than one table per relationship type, so that a traversal never has to `UNION` across an
open-ended number of tables as new predicates are introduced by
[24 — Plugin Framework](24-plugin-framework.md) Capabilities. The `origin` column
(`explicit`/`inferred`) is the single bit that lets [10 — Semantic Filesystem](10-semantic-filesystem.md)
distinguish "the user put this here" from "the system inferred this belongs here" — it is the
mechanism by which user-created folders are preserved rather than silently reorganized.
**`object_versions`** is an immutable, hash-linked chain per object (structurally a commit graph)
written once per Object Write Transaction and never mutated.

## Algorithms

**Upsert / version-chain maintenance.** Every write goes through
[28](28-storage-engine.md#algorithms)'s Object Write Transaction; at the schema level, the
invariant enforced is: a `semantic_objects` row's `version_id` always points to the newest row in
`object_versions` for that `object_id`, and every `object_versions` row except the root has a
`parent_version` that resolves to exactly one ancestor — the schema equivalent of a linked list
that can be walked backward for rollback and forward for audit.

**Graph traversal (Knowledge Graph queries, §09).** A multi-hop query such as "everything
connected to the Hawaii trip within two hops" is a recursive CTE. Edges are stored directionally
(`subject_id` is the part, `object_id` is the whole/anchor it relates to — e.g. `(photo,
part_of_trip, trip)`), but "everything related to X" is a connectivity query, not a directed
reachability query: it must walk edges in *both* directions from the anchor, or it will only ever
discover objects the anchor points to and miss the far more common case of objects that point *at*
the anchor (as every `part_of_trip` edge does). The traversal therefore unions a forward step and a
backward step at every hop:

```sql
WITH RECURSIVE reachable(object_id, depth) AS (
    SELECT object_id, 0 FROM semantic_objects WHERE object_id = :trip_id
  UNION ALL
    -- forward: follow edges FROM an already-reached node (anchor is the subject)
    SELECT e.object_id, r.depth + 1
    FROM edges e JOIN reachable r ON e.subject_id = r.object_id
    WHERE r.depth < :max_hops AND NOT e.tombstone
  UNION ALL
    -- backward: follow edges TO an already-reached node (anchor is the object, as with part_of_trip)
    SELECT e.subject_id, r.depth + 1
    FROM edges e JOIN reachable r ON e.object_id = r.object_id
    WHERE r.depth < :max_hops AND NOT e.tombstone
)
SELECT DISTINCT so.* FROM semantic_objects so
JOIN reachable r ON so.object_id = r.object_id
WHERE so.owner_id = :caller_owner_id;   -- ACL/shard filter, never optional
```

Both `UNION ALL` arms feed the same `reachable` accumulator, so a node reached via either direction
is not re-expanded once seen (`DISTINCT` in the final `SELECT`, and any implementation should track
visited `object_id`s within the recursive term itself to avoid re-walking a cycle). Predicate
direction is still preserved and returned to the caller (via `edges.predicate`, `edges.subject_id`,
`edges.object_id` on each hop) for callers that need to reason about the relationship's meaning —
only the *reachability* test, not the semantic direction, is treated as undirected.

**Semantic Filesystem folder-view queries (§10).** The same traversal, filtered to a query's
predicate/type constraints and merged with a vector similarity search, re-ranked, and returned as
directory entries rather than graph nodes — see the worked "vacation" query in
[10 — Semantic Filesystem](10-semantic-filesystem.md#algorithms). No separate storage is read;
only the presentation differs.

**Sharding/routing.** A request is routed to a shard by hashing `owner_id` (§Trade-offs) before
any query executes; cross-shard edges (a shared album between two users) are the one case routed
twice, once per owner, with both sides' ACLs independently checked.

## Interfaces / APIs

This schema is not exposed directly to Plugins or Agents; it is queried exclusively through the
narrow [28 — Storage Engine](28-storage-engine.md#interfaces--apis) API
(`get_object`, `query_edges`, `vector_search`) and the higher-level query surfaces of
[09](09-knowledge-graph.md) and [10](10-semantic-filesystem.md). This document exists so that
those two subsystems' implementers share one schema rather than inventing parallel ones.

## Schema (DDL)

```sql
-- Current-version snapshot of every Semantic Object.
CREATE TABLE semantic_objects (
    object_id       UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    object_type     TEXT NOT NULL,             -- 'photo','video','document','email','message',
                                                -- 'calendar_event','hotel_booking','expense',
                                                -- 'note','receipt','trip','collection','person', ...
    embedding       VECTOR(1536),               -- ANN-indexed; NULL for objects with no semantic content
    metadata        JSONB NOT NULL DEFAULT '{}',
    acl             JSONB NOT NULL,             -- capability-scoped ACL, see 15-security-architecture.md
    blob_hash       TEXT,                       -- FK (logical) into Storage Engine CABS; NULL if no payload
    version_id      BIGINT NOT NULL,            -- FK -> object_versions.version_id (current head)
    owner_id        UUID NOT NULL,              -- shard key, §Trade-offs
    device_origin   UUID NOT NULL,              -- device that authored the current version
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ                 -- tombstone; hard-delete happens only via GC, see 28
);

CREATE INDEX idx_objects_owner_type   ON semantic_objects (owner_id, object_type);
CREATE INDEX idx_objects_embedding    ON semantic_objects USING hnsw (embedding vector_cosine_ops);
CREATE INDEX idx_objects_metadata_gin ON semantic_objects USING gin (metadata jsonb_path_ops);

-- Relationship / edge table: the physical substrate of the Knowledge Graph (09).
-- `tombstone`/`version_vector` are what 09 §5.4's CRDT (grow-with-tombstones) merge and
-- undoable-edge-deletion guarantee (09 §10, Design Invariant 2) actually run against: a
-- deletion sets `tombstone = true` rather than issuing a physical DELETE, and
-- `version_vector` lets concurrent writers on different devices (21) merge without a
-- late-arriving insert silently resurrecting a deletion.
CREATE TABLE edges (
    edge_id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    subject_id      UUID NOT NULL REFERENCES semantic_objects(object_id),
    predicate       TEXT NOT NULL,              -- 'photographed_at','part_of_trip','member_of',
                                                 -- 'sent_by','attended_by','booked_for', ...
    object_id       UUID NOT NULL REFERENCES semantic_objects(object_id),
    weight          REAL NOT NULL DEFAULT 1.0,  -- relevance/strength, interpreted by 09
    provenance      TEXT NOT NULL,              -- 'user_explicit','agent:trip-planner',
                                                 -- 'inferred:clip-embedding','inferred:gps-cluster'
    origin          TEXT NOT NULL CHECK (origin IN ('explicit','inferred')),
    confidence      REAL CHECK (confidence BETWEEN 0 AND 1),  -- NULL for explicit edges
    owner_id        UUID NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at      TIMESTAMPTZ,                -- inferred edges may decay; explicit edges never set this
    tombstone       BOOLEAN NOT NULL DEFAULT false,  -- soft-deleted, never resurrected — 09 §5.4/§10
    version_vector  JSONB NOT NULL DEFAULT '{}'::jsonb  -- {replica_id: counter, ...} for CRDT merge
);

CREATE INDEX idx_edges_subject       ON edges (subject_id, predicate) WHERE NOT tombstone;
CREATE INDEX idx_edges_object        ON edges (object_id, predicate) WHERE NOT tombstone;
CREATE INDEX idx_edges_owner_org     ON edges (owner_id, origin) WHERE NOT tombstone;
CREATE INDEX idx_edges_tombstoned_at ON edges (owner_id) WHERE tombstone;  -- retention/GC sweep, 28

-- Immutable, hash-linked version history. Feeds 33-rollback-recovery.md.
CREATE TABLE object_versions (
    version_id      BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id       UUID NOT NULL REFERENCES semantic_objects(object_id),
    parent_version  BIGINT REFERENCES object_versions(version_id),
    blob_hash       TEXT,
    metadata_diff   JSONB,
    actor           TEXT NOT NULL,              -- capability token id / agent id
    wal_offset      BIGINT NOT NULL,             -- ties back to 28's Storage WAL
    hlc_timestamp   TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_versions_object_time ON object_versions (object_id, created_at DESC);

-- Explicit, user-created "folders" — never AI-reorganized. See 10-semantic-filesystem.md.
CREATE TABLE collections (
    collection_id   UUID PRIMARY KEY REFERENCES semantic_objects(object_id),
    display_name    TEXT NOT NULL,
    is_user_created BOOLEAN NOT NULL DEFAULT true,
    sort_order      JSONB
);
-- Membership is just an edge: (member_object) -[member_of, origin='explicit']-> (collection_id)
```

### Worked example: the vacation scenario

```sql
-- A photo taken in Maui, embedded and geotagged.
INSERT INTO semantic_objects (object_id, object_type, embedding, metadata, acl, blob_hash,
                               version_id, owner_id, device_origin, updated_at)
VALUES ('7c1e...photo', 'photo', '[0.014, -0.221, ...]',
        '{"gps": [20.7984, -156.3319], "taken_at": "2026-06-14T15:32:00Z", "camera": "iPhone 16"}',
        '{"read": ["owner:joao"], "share": []}',
        'blake3:9f2a...', 40231, 'joao-owner-uuid', 'joao-phone-uuid', now());

-- A hotel booking Semantic Object created by the Travel Capability.
INSERT INTO semantic_objects (object_id, object_type, metadata, acl, version_id, owner_id,
                               device_origin, updated_at)
VALUES ('a91b...hotel', 'hotel_booking',
        '{"confirmation": "HYP-88213", "checkin": "2026-06-12", "checkout": "2026-06-18",
          "property": "Wailea Beach Resort"}',
        '{"read": ["owner:joao"], "share": []}', 40198, 'joao-owner-uuid', 'joao-laptop-uuid', now());

-- The trip itself, and the edges that tie the photo and booking to it.
INSERT INTO edges (subject_id, predicate, object_id, weight, provenance, origin, confidence, owner_id)
VALUES
  ('7c1e...photo', 'photographed_at', 'loc-maui-uuid', 1.0, 'inferred:gps-cluster', 'inferred', 0.94, 'joao-owner-uuid'),
  ('7c1e...photo', 'part_of_trip',    'trip-hawaii-uuid', 1.0, 'agent:trip-assembler', 'inferred', 0.88, 'joao-owner-uuid'),
  ('a91b...hotel', 'part_of_trip',    'trip-hawaii-uuid', 1.0, 'user_explicit', 'explicit', NULL, 'joao-owner-uuid');
```

The photo and the hotel booking never reference each other directly; both are tied to the trip
object, which is how "everything related to my vacation" resolves as a two-hop traversal from a
single anchor rather than an all-pairs join.

## Sharding and Partitioning

The primary shard key is `owner_id`: every table above is range- or hash-partitioned by
`owner_id` first, which keeps a single user's — and, for the common case, a single device's —
working set together and makes per-user export/delete (a
[16 — Privacy Architecture](16-privacy-architecture.md) requirement) a partition-local operation
rather than a scatter-gather across the whole store. Within a user's partition:

- `edges` is further hash-partitioned by `subject_id`, since the dominant access pattern is "all
  edges for object X" (traversal fan-out); this keeps that lookup single-partition even for
  users with millions of edges.
- `object_versions` is time-partitioned (monthly), since access skews heavily toward recent
  history and old partitions are the natural unit for the tiered-retention compaction described
  in [28](28-storage-engine.md#algorithms).
- The `embedding` HNSW index is built per `(owner_id, object_type)` sub-shard so a photo-heavy
  library never inflates ANN search latency for a user's small set of documents, and so a
  multi-device sync (§28) only needs to rebuild the shard actually affected.
- Each device holds a full replica of its user's shard (not a subset), per the replication model
  in [28 — Storage Engine](28-storage-engine.md#algorithms) and
  [21 — Distributed Execution](21-distributed-execution.md); "per-device" is a replication
  topology, not a separate partitioning axis.

## Security Considerations

Row-level access control is enforced twice: structurally, every table carries `owner_id` and no
query is issued without it (§Algorithms); semantically, the `acl` JSONB on `semantic_objects` is
evaluated per-row by the Storage Engine before a row crosses a Trust Boundary (see
[28 — Storage Engine](28-storage-engine.md#security-considerations)), so partitioning by owner is
a performance optimization, never the actual security boundary. Certain predicates (e.g. health-
or financial-adjacent relationships) can carry a stricter ACL than their endpoints individually,
checked at edge-read time, not inherited implicitly from either endpoint. Columns holding
sensitive plaintext (metadata fields marked sensitive by [16](16-privacy-architecture.md)) are
additionally column-encrypted using the DEK/KEK hierarchy defined in
[28 §Security](28-storage-engine.md#security-considerations).

## Failure Modes

- A schema migration adding a new `object_type` or predicate ships without updating an index,
  degrading a hot query path to a sequential scan under load.
- Hard deletion of a `semantic_objects` row while `edges` still reference it as `subject_id` or
  `object_id` produces an orphaned edge (mitigated by foreign keys plus a soft-delete-first
  policy — see §Recovery).
- A broken `parent_version` chain (corrupted write, bad migration) makes rollback for that object
  impossible past the break point.
- Skewed partitioning: a single user with an unusually large trip graph (tens of thousands of
  photos) can create a hot shard.

## Recovery Mechanisms

Foreign keys on `edges` and `object_versions` prevent the common case of orphaning outright;
where a soft-delete (`deleted_at`) is used instead of a hard delete, an orphan sweeper job runs
during the same idle window as [28](28-storage-engine.md#algorithms)'s GC pass and either
restores the tombstoned object (if a live edge still needs it) or cascades the edge removal. A
broken version chain is repaired by replaying [28](28-storage-engine.md)'s Storage WAL, which is
the durable source the `object_versions` table is itself derived from — the table is a cache of
the log, not the log itself. Hot shards are rebalanced by moving a user's partition to a
larger/dedicated shard transparently; because the shard key is stable (`owner_id`), rebalancing
never changes an `object_id`.

## Performance Analysis

Point lookups (`get_object`) are O(1) via the primary key. Bounded-depth traversal (the common
case: 1–3 hops, as in the vacation example) is O(fan-out^depth) against the `idx_edges_subject`
index and stays sub-100ms for realistic personal graphs (tens of thousands of edges) per the
targets in [36 — Performance Benchmarks](36-performance-benchmarks.md); unbounded-depth traversal
is deliberately not offered as a primitive — [09](09-knowledge-graph.md) always caps `max_hops`.
Vector search cost is governed by the per-`(owner_id, object_type)` HNSW shard size, not total
corpus size. The JSONB GIN index on `metadata` accelerates the common filter predicates
(date ranges, camera model, confirmation numbers) without requiring a fully normalized column per
object type, at the query-planner cost discussed in §Trade-offs.

## Trade-offs

JSONB metadata buys schema-on-read flexibility — new Semantic Object types introduced by
[24 — Plugin Framework](24-plugin-framework.md) Capabilities need no migration — at the cost of a
less optimizable query plan than typed columns; hot fields (GPS, timestamps, confirmation
numbers) are promoted to expression indexes rather than typed columns to recover most of the
lost performance without losing the flexibility. A single `edges` table for every predicate
keeps traversal queries uniform and avoids an ever-growing `UNION ALL` as new predicates appear,
at the cost of a wider table than a normalized per-predicate schema would need — mitigated by the
`(subject_id, predicate)` and `(object_id, predicate)` composite indexes, which make the
single-table design perform comparably to per-predicate tables for the actual access pattern.
Per-user sharding is simple and privacy-aligned (a user's data lives in one place, deletable as a
unit) but complicates the rarer case of cross-user relationships (a shared album, a joint
calendar event); those are modeled as edges that exist redundantly in both owners' shards with
independent ACL checks rather than a single cross-shard edge, trading storage duplication for
partition independence.

## Testing Strategy

Referential-integrity fuzzing generates random edge/version insert-delete sequences and asserts
no orphan survives a GC pass. Migration tests run every schema change against a snapshot of the
worked vacation-scenario fixture above and assert the traversal and vector-search queries in
§Algorithms still return the same result set. Load tests populate a single-user partition with
synthetic graphs at 10x and 100x the expected personal-scale edge count to validate the
sharding/partitioning choices in §Sharding and Partitioning before they are needed in production.
Partition rebalancing is drilled by moving a live synthetic user shard mid-query-load and
asserting no query observes a torn read. These feed the shared harness in
[35 — Testing Strategy](35-testing-strategy.md).

---
*Next: [30 — IPC Framework](30-ipc-framework.md).*
