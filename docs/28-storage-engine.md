# Storage Engine

## Purpose

The Storage Engine is the physical persistence layer underneath every Semantic Object in
Hyperion. It owns four things and exactly four things: a content-addressed **blob store** for
binary payloads, a **graph index** for relationships between objects, a **vector index** for
embeddings, and a **metadata/permissions store** for everything else an object carries. It does
not know what a "vacation" is, what a knowledge-graph traversal means, or what a folder looks
like — those are the concerns of [09 — Knowledge Graph](09-knowledge-graph.md) and
[10 — Semantic Filesystem](10-semantic-filesystem.md), both of which are built entirely on the
primitives this document defines. The concrete relational/graph schema realized inside the
metadata and graph stores described here is specified in
[29 — Database Schema](29-database-schema.md); this document is about the engine that makes
writes to that schema atomic, durable, encrypted, replicated, and boundedly sized.

## Motivation

Every higher layer in Hyperion — the [Intent Engine](05-intent-engine.md), the
[Memory Engine](08-memory-engine.md), the [Knowledge Graph](09-knowledge-graph.md), the
[Semantic Filesystem](10-semantic-filesystem.md) — makes the same four demands of storage
simultaneously for a single logical write: store the bytes, update the relationships, update the
searchable meaning, and update who is allowed to see it. A traditional stack solves this with a
filesystem for bytes, a separate database for metadata, a separate vector database bolted on
later, and no first-class notion of relationships at all — four systems with four consistency
models, glued together by application code that inevitably drifts out of sync. Hyperion cannot
afford that drift: Design Invariant 2 in [02 — Core Architecture](02-core-architecture.md#4-design-invariants)
requires that *everything is undoable or versioned*, which is only true if a single write to a
Semantic Object is atomic across all four stores. A photo whose blob is written but whose
embedding never lands is invisible to every future "find my vacation photos" query forever; a
graph edge written without its metadata counterpart is a dangling reference no traversal can
explain. The Storage Engine exists to make "partially written Semantic Object" an impossible
state, not a rare one.

## Architecture

```
                         Object Write Transaction (OWT)
                                     │
                                     ▼
 ┌───────────────────────────────────────────────────────────────────────┐
 │                            Storage WAL                                │
 │   append-only, fsync'd, single source of truth for commit order       │
 └───────────────┬───────────────────┬───────────────────┬───────────────┘
                 │ redo/apply        │ redo/apply        │ redo/apply
                 ▼                   ▼                   ▼
   ┌─────────────────────┐ ┌──────────────────┐ ┌───────────────────────┐
   │ Content-Addressed    │ │  Graph Index      │ │  Metadata/Permission  │
   │ Blob Store (CABS)    │ │  (edges, §29)     │ │  Store (objects, §29) │
   │ hash → encrypted     │ │  subject→object   │ │  JSONB + ACL + version│
   │ chunk set            │ │  adjacency        │ │  pointer              │
   └─────────────────────┘ └──────────────────┘ └───────────────────────┘
                 │                   │                   │
                 └─────────┬─────────┴─────────┬─────────┘
                           ▼                   ▼
                 ┌───────────────────┐ ┌────────────────────┐
                 │   Vector Index    │ │   Version Chain      │
                 │  (ANN, HNSW-class)│ │  object_versions §29 │
                 └───────────────────┘ └────────────────────┘
                           │
                           ▼
        ┌───────────────────────────────────────────────┐
        │  Sync/Replication Engine (per-device instance) │
        │  Merkle-diff over WAL segments, CRDT merge      │
        │  → 21-distributed-execution.md                  │
        └───────────────────────────────────────────────┘
```

The load-bearing architectural decision is that **the four stores are materialized views of the
WAL, not four independently-committed systems**. The WAL is the only place atomicity is enforced;
everything below it is a redo target that can be rebuilt by replaying the log. This sidesteps a
distributed two-phase commit across heterogeneous storage engines (blob store, graph index,
vector index, relational metadata) — a notoriously fragile pattern — in favor of the well-understood
single-writer-ahead-log model used by every serious embedded database, generalized across four
physical representations instead of one.

## Data Structures

**Blob descriptor** (Content-Addressed Blob Store): `{content_hash: BLAKE3, size, chunk_manifest:
[chunk_hash], encryption: {dek_wrapped, algorithm}, refcount}`. Large objects (photos, videos,
[local model weights](22-local-ai-runtime.md)) are content-defined-chunked so that a small edit
(cropping a photo, fine-tuning a model checkpoint) only writes the changed chunks.

**WAL record**: `{wal_offset, hlc_timestamp, actor_capability_token, object_id, prev_version_id,
new_version_id, blob_hash?, metadata_delta: JSON patch, edge_deltas: [EdgeOp], embedding?:
float[]}`. One record is one Object Write Transaction; it is the unit of atomicity and the unit
of replication.

**Graph edge record**: mirrors the `edges` table defined in
[29 — Database Schema](29-database-schema.md#schema-ddl) — `subject_id, predicate,
object_id, weight, provenance, origin (explicit|inferred), confidence, expires_at, tombstone,
version_vector`. The last two are what make an edge deletion undoable and mergeable under
concurrent, multi-device writes rather than a silent physical removal (09 §5.4/§10). The Storage
Engine treats edges as opaque adjacency data; *interpreting* them (traversal, ranking, decay) is
[09 — Knowledge Graph](09-knowledge-graph.md)'s job.

**Vector index entry**: `{object_id, embedding, index_shard}`, held in an ANN structure
(HNSW-class graph) partitioned per `(owner_id, object_type)` per [29](29-database-schema.md#sharding-and-partitioning)
so that no single user's photo library forces a scan of another user's documents.

**Version record**: `{version_id, object_id, parent_version_id, blob_hash?, metadata_diff,
actor, wal_offset, hlc_timestamp}` — an immutable, hash-linked chain per object, structurally
identical to a commit graph. This chain is what [33 — Rollback & Recovery](33-rollback-recovery.md)
walks to produce a recovery point.

## Algorithms

**Write path (Object Write Transaction).** A caller (a Capability, an Agent, or the
[Semantic Filesystem](10-semantic-filesystem.md)'s write-back path) never touches the four stores
directly; it submits one OWT.

1. *Blob phase (optional).* If the OWT carries a binary payload, it is chunked, hashed, and
   encrypted (§Security) before the WAL is touched. Content addressing makes this phase safe to
   redo: writing the same bytes twice is a no-op refcount bump, and a blob written but never
   referenced by a committed WAL record is inert garbage, not corruption.
2. *WAL commit.* Exactly one WAL record is appended and durably flushed. This is the atomicity
   boundary — the moment after which the write is committed, and the moment before which it
   never happened. Optimistic concurrency is enforced here: the record includes the version the
   caller believed was current (`prev_version_id`); if another writer has since advanced the
   object's version pointer, the append is rejected and the caller retries against the new head
   (a compare-and-swap on the version pointer, not a lock).
3. *Apply phase.* Background appliers redo the record into the CABS refcount table, the graph
   index, the metadata store, and the vector index. Until applied, reads for that object are
   served by consulting a pending-writes overlay keyed by WAL offset, guaranteeing read-your-writes
   without requiring the apply phase to be synchronous.
4. *Version phase.* A new `object_versions` row is inserted and the object's version pointer is
   advanced.

**Read path.** Resolve `object_id` → consult pending overlay for any unapplied WAL records → fall
back to the metadata store for the current version pointer → assemble the response from metadata
store + CABS (if `blob_hash` present) + vector index (if embedding requested) + graph index (if
edges requested). A read never touches more stores than the caller asked for.

**Sync/merge (per device).** Two devices exchange a Merkle tree keyed by WAL segment hash, scoped
to the objects they are authorized to see (§Security). Missing segments are transferred; blob
chunks are fetched by hash only if not already locally present (deduplicating across devices, not
just within one). Concurrent, causally-unrelated WAL records for the same object (a genuine fork)
are merged field-by-field using hybrid-logical-clock order for scalar metadata (last-writer-wins)
and as an OR-Set union for graph edges (additions and removals commute naturally, so "user added a
`part_of_trip` edge on the phone" and "agent added a `photographed_at` edge on the laptop" both
survive). A fork in the *blob* itself (two genuinely different edits to the same photo) is the one
case that cannot be silently merged; it is surfaced as a version-chain branch that
[18 — Explainability & Trust](18-explainability-and-trust.md) presents to the user as an explicit
choice, never resolved by silent overwrite. Transport rides on
[19 — Networking Stack](19-networking-stack.md); orchestration of which device is authoritative
for which in-flight computation is [21 — Distributed Execution](21-distributed-execution.md)'s
concern — the Storage Engine only guarantees that whichever device computes first, the *storage*
converges.

**Garbage collection / compaction.** Runs during idle/low-power windows
(see [37 — Scalability Roadmap](37-scalability-roadmap.md)):
- Blob GC is reference-counted mark-and-sweep: a blob is collectible when its refcount is zero
  *and* it is not pinned by any version inside the retention window.
- Version retention is tiered: verbatim diffs for the last *N* versions or *T* days (both
  configurable), collapsing into periodic snapshots beyond that, and full pruning only past the
  user's configured retention limit — never past an active legal hold or an unresolved rollback
  point (see [33](33-rollback-recovery.md)).
- Inferred edges below a confidence threshold and past their provenance TTL are pruned by the
  Storage Engine on a schedule set by [09](09-knowledge-graph.md); explicit edges (§10, user- or
  agent-on-behalf-of-user-created) are never auto-pruned.
- The ANN index is periodically rebuilt/compacted, since HNSW-class structures degrade under a
  high ratio of soft deletes to live vectors.

## Interfaces / APIs

The Storage Engine's API is deliberately narrow and is consumed only by L3/L4 subsystems
([09](09-knowledge-graph.md), [10](10-semantic-filesystem.md), [08 — Memory Engine](08-memory-engine.md))
— never directly by a Plugin or Agent, consistent with the capability-security model in
[02 §5](02-core-architecture.md#5-capability-security-as-the-unifying-security-model):

```
begin_txn(capability_token)                         -> TxnHandle
put_object(txn, ObjectWriteTxn)                      -> ObjectVersion
get_object(capability_token, object_id, version?)    -> SemanticObjectRecord
query_edges(capability_token, subject_id?, predicate?, object_id?, limit) -> Edge[]
vector_search(capability_token, embedding, k, filter?) -> [(object_id, score)]
subscribe(capability_token, scope)                   -> EventStream   (see 31-event-system.md)
commit_txn(txn) / abort_txn(txn)
```

All calls are dispatched over [30 — IPC Framework](30-ipc-framework.md) and every call is
re-checked against the ACL in the metadata store regardless of what the caller believes it is
authorized to do (§Security).

## Pseudocode: Object Write Transaction

```
function put_object(txn, owt):
    if owt.blob_payload is not None:
        chunks = content_defined_chunk(owt.blob_payload)
        for chunk in chunks:
            hash = blake3(chunk)
            if not cabs.exists(hash):
                dek = generate_dek()
                cabs.write(hash, encrypt(chunk, dek))
                keystore.wrap_and_store(hash, dek)     # envelope encryption, see §Security
            cabs.incref(hash)
        owt.blob_hash = manifest_hash(chunks)

    record = WalRecord(
        object_id      = owt.object_id or new_uuid(),
        prev_version    = owt.expected_version,
        new_version     = next_version_id(),
        blob_hash       = owt.blob_hash,
        metadata_delta  = owt.metadata_delta,
        edge_deltas     = owt.edge_deltas,
        embedding       = owt.embedding,
        actor           = txn.capability_token.id,
        hlc_timestamp   = hlc.now(),
    )

    # atomicity boundary: the fsync'd append is the commit point
    offset = wal.append_and_fsync(record)

    if not cas(object_version_pointer[record.object_id],
               expected=record.prev_version, new=record.new_version):
        wal.mark_aborted(offset)
        raise ConcurrentWriteConflict(record.object_id)   # caller retries

    async_apply_queue.push(record)          # metadata store, graph index, vector index
    pending_overlay.add(record)             # guarantees read-your-writes until applied
    return record.new_version


function apply_worker():
    for record in async_apply_queue:
        metadata_store.upsert(record.object_id, record.metadata_delta, record.new_version)
        graph_index.apply(record.edge_deltas)
        if record.embedding is not None:
            vector_index.upsert(record.object_id, record.embedding)
        version_table.insert(record.new_version, record.object_id, record.prev_version,
                              record.blob_hash, record.wal_offset)
        pending_overlay.remove(record)
```

## Security Considerations

Every call, including internal calls from [09](09-knowledge-graph.md) and
[10](10-semantic-filesystem.md), carries a capability token scoped to a specific object, object
type, or query shape (per [15 — Security Architecture](15-security-architecture.md)); the ACL
stored alongside each object in the metadata store is re-evaluated on every `get_object`,
`query_edges`, and `vector_search` call, never cached across a Trust Boundary. This closes the
confused-deputy risk where the [Semantic Filesystem](10-semantic-filesystem.md)'s POSIX compat
shim could otherwise act with its own broad privilege on behalf of a less-privileged legacy app —
the shim must forward the originating identity, not substitute its own.

Encryption at rest uses envelope encryption tied to
[16 — Privacy Architecture](16-privacy-architecture.md): each blob chunk is encrypted with a
per-object Data Encryption Key (AES-256-GCM), the DEK is wrapped by a per-workspace Key
Encryption Key held in the device's secure keystore/TPM, and the KEK is itself wrapped under a
master key derived from user credentials. The metadata store and version table are encrypted at
rest under the same hierarchy. Embeddings are computed from plaintext exclusively on-device by
default (Design Invariant 3, local-first) and the vector index is encrypted at rest, but must be
decrypted transiently in memory to serve ANN search — this in-memory exposure window is
mitigated by running the vector index inside the same Trust Boundary as the metadata store, never
inside a Plugin's sandbox. Key loss (device wipe, forgotten passphrase with no escrow configured)
is an explicit, disclosed data-loss risk, not silently masked.

## Failure Modes

- WAL append fails (disk full, storage medium failure) → write rejected before any store is
  touched; the caller observes a normal failure, not partial state.
- Process crash between blob write and WAL append → an orphaned, unreferenced blob; harmless,
  collected by the next GC pass.
- Process crash between WAL append and apply phase → the WAL record exists but the metadata/graph/
  vector stores have not caught up; detected and repaired by replay on restart (§Recovery).
- Network partition during sync → devices diverge; reconciled on reconnect via the Merkle-diff
  merge algorithm (§Algorithms), with irreconcilable blob forks surfaced to the user rather than
  silently resolved.
- ANN index corruption (bad shutdown mid-compaction) → detected via a checksum on the index
  header; rebuilt from the metadata store's embedding column, which is always the durable source.
- Clock skew between devices → HLC ordering degrades to physical-clock-order fallback with a
  wider conflict-detection window, trading a higher false-conflict rate for correctness.

## Recovery Mechanisms

On restart, the engine replays the WAL from the last confirmed apply checkpoint, re-running the
apply phase for any record whose effects are not yet visible in the metadata/graph/vector stores —
a standard redo-log recovery, generalized across four physical targets instead of one. Version
chains give [33 — Rollback & Recovery](33-rollback-recovery.md) a rollback primitive for free: a
recovery point *is* a `version_id`, and rolling back is "advance the pointer to an older
version_id and append a new WAL record recording that fact" rather than a bespoke undo mechanism.
Content addressing gives self-verifying integrity: any blob's hash is recomputed and checked on
read, so silent bit-rot is detected rather than served. Cross-device reconciliation after a long
partition uses the same Merkle-diff sync path used for routine convergence — there is no separate
disaster-recovery code path to keep correct.

## Performance Analysis

The write path's latency budget is dominated by the WAL fsync (single-digit milliseconds on flash
storage), not by the four downstream applies, which happen asynchronously and are pipelined —
this is why the WAL-as-source-of-truth design was chosen over synchronous four-way commit. Group
commit batches concurrent OWTs into a single fsync under load, trading a small latency increase
(bounded to a few milliseconds) for an order-of-magnitude throughput gain, matching the
sub-second workspace generation targets in
[36 — Performance Benchmarks](36-performance-benchmarks.md). Point reads (`get_object` by id) are
O(1) via the metadata store's primary index; vector search is approximate-nearest-neighbor,
sub-linear in corpus size via the HNSW-class structure, and bounded further by per-`(owner_id,
object_type)` sharding so a laptop with a modest photo library never pays the cost of a
hypothetical enterprise-scale corpus. Sync bandwidth is dominated by genuinely new bytes only:
content-defined chunking means editing one paragraph of a large document or cropping one photo
transfers a chunk, not the whole object.

## Trade-offs

Using the WAL as the sole atomicity boundary buys crash-consistency across four heterogeneous
stores without distributed 2PC, at the cost of a replay window on cold start after an unclean
shutdown — bounded by periodic checkpointing of the apply-queue watermark. Content-addressed
storage buys deduplication and tamper-evidence, at the cost of a hashing pass on every write,
mitigated by streaming/incremental hashing so large blobs (video, model weights) never require a
full buffered read before the write can begin. Field-level CRDT merge for metadata and edges buys
availability during multi-device partitions (PACELC: partition-tolerant and available, eventually
consistent) at the cost of pushing genuine blob-content conflicts to the user rather than
resolving them automatically — judged an acceptable trade against Design Invariant 4 (every
autonomous action explainable) and Invariant 1 (no silent authority), since silently picking a
winner would itself be an unexplained autonomous decision about the user's data. Co-locating
embeddings inside the same engine as blobs, metadata, and graph edges (rather than a bolt-on
vector database) avoids duplicate storage and dual-write drift, at the cost of the vector index
needing engine-specific compaction rather than reusing an off-the-shelf vector DB's tooling.

## Testing Strategy

Crash-consistency is tested by fault injection at each of the four write-path phases (kill the
process between blob write and WAL append, between WAL append and apply, mid-apply-phase) and
asserting that replay always converges to a state consistent with the last durably committed WAL
record — never partial. Reference-counting invariants for blob GC are checked by property tests
that assert a blob is never collected while any reachable version still points to it. Sync is
exercised under chaos conditions: devices held offline for simulated weeks, then reconnected with
large divergent histories, asserting Merkle-diff converges to the same merged state regardless of
reconnection order. Encryption key rotation is drilled end-to-end (rotate KEK, verify all DEKs
re-wrap without a data re-encryption pass). Concurrency stress tests run many simulated writers
against the same object to validate the compare-and-swap version-pointer path never lets two
writers both believe they won. All of the above feed the shared harness described in
[35 — Testing Strategy](35-testing-strategy.md).

---
*Next: [29 — Database Schema](29-database-schema.md).*
