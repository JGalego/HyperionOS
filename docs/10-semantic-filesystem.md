# Semantic Filesystem

## Purpose

The Semantic Filesystem is the *view layer* that lets a user, a legacy application, or a native
Hyperion Capability ask for information the way a person actually thinks about it — "everything
related to my vacation" — and receive a coherent, navigable result, without Hyperion maintaining
a second copy of the data to make that possible. It sits directly on top of
[09 — Knowledge Graph](09-knowledge-graph.md) (which owns the graph model: nodes, edges,
embeddings, reasoning-search) and [28 — Storage Engine](28-storage-engine.md) (which owns the
physical bytes, per [29 — Database Schema](29-database-schema.md)). The Semantic Filesystem owns
neither the graph model nor the physical storage — it owns the translation between "a query" and
"a folder," and between "a legacy path" and "a Semantic Object."

## Motivation

[01 — Vision & Philosophy](01-vision-and-philosophy.md#4-primary-design-philosophy) establishes
that the unit of stored information is a Semantic Object, not a file in a folder, and that
folders are a compatibility view, not the underlying model (see
[02 — Core Architecture](02-core-architecture.md#semantic-object)). That principle is easy to
state and hard to make real: the moment a real filesystem stops being the source of truth, every
legacy application that calls `open()`, every script that does `ls`, and every user habit built
over decades of "where did I save that?" needs an answer that does not feel broken. The
Semantic Filesystem is that answer. It must make "show me everything related to my vacation"
resolve to a live, correct assembly of photos, videos, emails, map pins, hotel bookings, messages,
expenses, calendar events, notes, and receipts — objects that may live in entirely different
Capabilities and have never been co-located by any human — while simultaneously making a folder a
user dragged three files into behave exactly as durably as it would on any traditional OS. Losing
either property breaks trust: lose the first and Hyperion is just a filesystem with a search box;
lose the second and a user's explicit organizational effort evaporates into "the AI reorganized my
files," which Design Invariant 1 (no silent authority) in
[02 — Core Architecture](02-core-architecture.md#4-design-invariants) forbids outright.

## Architecture

```
        Legacy POSIX App              Native Hyperion Capability / Agent
        (Compatibility Layer,               (Intent Engine, 05 / Agent
         see 27-compatibility-layer.md)       Runtime, see 11)
              │  open("/Vacation/IMG_0231.jpg")   │  query("everything related to my vacation")
              ▼                                    ▼
     ┌───────────────────────┐          ┌─────────────────────────────┐
     │  POSIX Compat Shim     │          │  Semantic Query API          │
     │  (path <-> object_id   │          │  (NL/structured query in,    │
     │   resolution, FUSE-    │          │   VirtualFolder out)         │
     │   class interface)     │          │                              │
     └───────────┬───────────┘          └──────────────┬──────────────┘
                 │                                       │
                 └───────────────┬───────────────────────┘
                                  ▼
                   ┌───────────────────────────────────┐
                   │   Query Resolver /                 │
                   │   Virtual Directory Materializer    │
                   │   (graph traversal + vector search  │
                   │    + rank/merge + ACL filter)        │
                   └───────────────┬───────────────────┘
                                  │
              ┌───────────────────┼───────────────────────┐
              ▼                                            ▼
   ┌─────────────────────────┐                 ┌─────────────────────────┐
   │ 09 — Knowledge Graph      │                 │ 28 — Storage Engine        │
   │ (edges, embeddings,       │  ◄────────────► │ (blobs, metadata, ACL,     │
   │  reasoning-search)        │   same tables,   │  version chain, per 29)    │
   │                           │   see 29-schema  │                            │
   └─────────────────────────┘                 └─────────────────────────┘
```

Both entry points — the POSIX shim and the native Semantic Query API — terminate in the same
Query Resolver, so a legacy app browsing a synthesized directory and a native Capability running
a structured query see a provably consistent view of the same objects; there is exactly one
resolution path, not two.

## Data Structures

**VirtualFolder** — the materialized result of a query, not a stored object:
`{query_spec, member_object_ids: [ObjectID], materialized_at, ttl, sort_order, snapshot_token}`.
The `snapshot_token` matters more than it looks: it is what lets a POSIX `opendir()`/`readdir()`
sequence see a stable listing for the lifetime of the handle even though the underlying graph may
change mid-traversal (§Failure Modes).

**PathMapping cache** — `{synthesized_path <-> object_id}`, persistent across sessions so that a
legacy app's cached path or inode number keeps resolving correctly rather than shuffling every
time the Query Resolver re-runs. Populated lazily as paths are synthesized, never precomputed for
the whole graph.

**Collection** — a genuine Semantic Object of `object_type = 'collection'` (see
[29 — Database Schema](29-database-schema.md#schema-ddl)) plus explicit `member_of` edges with
`origin = 'explicit'`. This is the entire mechanism by which a user-created folder is preserved:
it is not a special case in the Semantic Filesystem's code, it is an ordinary Semantic Object
with ordinary explicit edges, which means it survives graph GC, re-indexing, and AI-driven
re-organization exactly as durably as any other explicitly authored fact.

**QuerySpec** — the structured form a natural-language request like "everything related to my
vacation" is compiled into by the [Intent Engine](05-intent-engine.md): an anchor object or
concept (the "Hawaii trip"), a hop-depth bound, optional type filters, and an optional embedding
for semantic (rather than purely relational) matches.

## Algorithms

**Query resolution ("query-as-navigation").** A request — typed, spoken, or a synthesized path
under the POSIX shim's mount root — is resolved in four stages: (1) the
[Intent Engine](05-intent-engine.md) and [Context Engine](06-context-engine.md) compile it into a
QuerySpec, resolving "my vacation" to a concrete anchor object via recent
[Context Bundle](07-context-propagation.md) state or a fuzzy name match; (2) the Query Resolver
issues a bounded-hop graph traversal from the anchor through
[09 — Knowledge Graph](09-knowledge-graph.md) (reusing exactly the recursive-CTE pattern shown in
[29 — Database Schema](29-database-schema.md#algorithms)) and, in parallel, a vector similarity
search for objects that are topically related but never explicitly linked (a journal entry that
mentions the trip but was never tagged); (3) results are merged, deduplicated by `object_id`, and
ranked; (4) the ranked set is wrapped into a VirtualFolder and, if a POSIX view was requested,
each member is given a synthesized path.

**Path synthesis (POSIX compat shim).** Deterministic, not creative: `{object_type}/{date or
title}` with a collision suffix (`-2`, `-3`, ...) when two objects would otherwise synthesize the
same leaf name. The PathMapping cache is consulted first — an object that has been assigned a
path before keeps that path — so that stable identifiers legacy tooling relies on (inode number,
absolute path) do not drift between one `ls` and the next.

**Write-back.** A legacy app's write is buffered by the compat shim and only committed as a
Storage Engine Object Write Transaction on `close()`/`fsync()`, giving POSIX's usual
close-to-open consistency without ever exposing a half-written Semantic Object (the same
atomicity boundary as [28](28-storage-engine.md#algorithms)'s OWT). What happens next depends on
*where* the write landed: a write into a real, user-created Collection folder (§Data Structures)
produces an explicit `member_of` edge, because the user's directory placement is itself an act of
organization worth preserving. A write into a *virtual*, query-materialized folder (e.g. an app
that insists on saving into "/Vacation/" where "Vacation" was synthesized by a live query, not
created by the user) does not fabricate a false explicit edge — the object is ingested normally,
Capabilities infer whatever relationships genuinely hold, and the PathMapping cache pins that
specific path to that specific object so the app's own bookkeeping keeps working even if the
object later drops out of the live query's result set.

**Folder preservation.** Creating a folder (`mkdir` under the compat shim, or "put these three
photos in a folder called Receipts") always creates a Collection Semantic Object plus explicit
edges — never a filesystem directory entry with no graph representation. This is what makes
"traditional folders remain available only for compatibility" true in both directions: the
compat shim can render it as a directory for legacy apps, and the Knowledge Graph can traverse it
as an ordinary explicit relationship, but there is only one underlying fact.

## Interfaces / APIs

```
fs.query(capability_token, spec: QuerySpec)                 -> VirtualFolder
fs.materialize(capability_token, virtual_folder_id)         -> [DirEntry]
fs.mkcollection(capability_token, name, parent?: CollectionID) -> CollectionObject
fs.add_to_collection(capability_token, object_id, collection_id)
fs.mount_posix(capability_token, root_spec)                 -> POSIX mountpoint (FUSE-class)
fs.resolve_path(capability_token, path)                     -> object_id
fs.write_back(capability_token, path, bytes)                -> ObjectVersion
```

Every call takes the caller's capability token and is subject to the same per-object ACL check
enforced at the Storage Engine layer (see
[28 — Storage Engine](28-storage-engine.md#security-considerations)); the Semantic Filesystem
adds no privilege of its own and cannot be used to see an object the caller could not otherwise
read. Legacy applications reach this API exclusively through the POSIX shim mounted by the
[27 — Compatibility Layer](27-compatibility-layer.md); native Capabilities and Agents
(see [11 — Agent Runtime](11-agent-runtime.md)) call `fs.query` directly.

## Pseudocode: Query Resolution and Write-Back

```
function resolve_query(capability_token, spec):
    anchor = intent_engine.resolve_anchor(spec.natural_language, context_bundle)
    relational = knowledge_graph.traverse(anchor, max_hops=spec.hop_bound,
                                           predicate_filter=spec.predicates)
    semantic = storage_engine.vector_search(capability_token, spec.embedding, k=spec.k)
    merged = rank_and_dedupe(relational + semantic)
    visible = [o for o in merged if acl_check(capability_token, o)]   # re-checked, never cached
    return VirtualFolder(
        query_spec=spec,
        member_object_ids=[o.object_id for o in visible],
        materialized_at=now(), ttl=spec.ttl or default_ttl,
        snapshot_token=new_snapshot_token(),
    )

function synthesize_paths(virtual_folder):
    for object_id in virtual_folder.member_object_ids:
        if path_mapping_cache.has(object_id):
            yield path_mapping_cache.get(object_id)
        else:
            candidate = f"{type_of(object_id)}/{title_or_date(object_id)}"
            path = disambiguate(candidate, path_mapping_cache)
            path_mapping_cache.put(object_id, path)
            yield path

function write_back(capability_token, path, bytes):
    object_id = path_mapping_cache.resolve(path) or new_object_id()
    txn = storage_engine.begin_txn(capability_token)
    version = storage_engine.put_object(txn, ObjectWriteTxn(
        object_id=object_id, blob_payload=bytes,
        metadata_delta=infer_metadata(bytes),
    ))
    storage_engine.commit_txn(txn)

    parent = containing_folder(path)
    if is_user_created_collection(parent):
        knowledge_graph.add_edge(object_id, 'member_of', parent.collection_id, origin='explicit')
    else:
        path_mapping_cache.pin(path, object_id)   # keep app's own bookkeeping stable
        # no fabricated explicit edge; ordinary inference pipeline runs asynchronously
    return version
```

## Security Considerations

Because the Query Resolver can surface an object that matches a query relationally or
semantically but that the caller is not permitted to see (a shared trip photo album where one
photo carries a stricter ACL), every result is re-checked against the caller's capability token
at materialization time, never assumed authorized because it appeared in a traversal — the same
per-row ACL enforcement described in
[29 — Database Schema](29-database-schema.md#security-considerations). The POSIX compat shim runs
legacy applications inside the Trust Boundary depth appropriate to that app
(see [03 — Kernel Architecture](03-kernel-architecture.md) and
[15 — Security Architecture](15-security-architecture.md)) and must forward the *originating*
identity on every call rather than acting as an ambient-authority proxy — a compromised legacy
app can only ever see what its own token permits, regardless of what the shim process itself
could technically reach. Path synthesis is treated as untrusted input on the way back in:
`write_back` validates the resolved path against the caller's mount root to prevent a legacy
app's `../../` traversal from being translated into a graph write against an object outside its
grant. Because one object can legitimately appear under many synthesized paths at once (a photo
in both "Vacation" and "2026" and "Beach Photos"), the shim presents this as POSIX hard-link
semantics rather than as duplicated files, which is the correct semantic and avoids accidentally
implying multiple independent copies exist.

## Failure Modes

- A VirtualFolder's underlying graph changes mid-listing (a background Agent adds a new photo to
  the trip while a legacy app is mid-`readdir()`), producing a listing that would otherwise
  flap from one syscall to the next.
- Two objects synthesize to the same leaf path and are not disambiguated deterministically across
  runs, breaking a legacy app's cached path.
- An ambiguous natural-language query ("my trip") resolves to the wrong anchor when the user has
  multiple candidate trips in context, returning a folder full of the wrong vacation.
- The compat shim crashes between buffering a write and committing it, risking data the legacy
  app believes was saved.
- The Query Resolver times out on an unusually large or deep graph (a power user with a decade of
  photos), leaving a legacy `ls` hanging.

## Recovery Mechanisms

Every POSIX `opendir()` is bound to a `snapshot_token` (§Data Structures): the directory listing
served for the lifetime of that handle is frozen to the graph state at open time, matching the
stability legacy applications assume, with changes visible only on the next `opendir()` — this is
the direct mitigation for the first failure mode above. Path collisions are resolved
deterministically (stable suffix ordering keyed by `object_id`, not creation order) so that
repeated runs of the disambiguation algorithm produce the same mapping without needing to persist
every possible collision in advance. Ambiguous anchor resolution falls back to presenting the
candidates for disambiguation rather than silently guessing, consistent with Design Invariant 4
(every autonomous action explainable) in
[02 — Core Architecture](02-core-architecture.md#4-design-invariants). Write-back uses the same
buffer-then-atomic-commit pattern as the Storage Engine's Object Write Transaction
(see [28](28-storage-engine.md#algorithms)), so a crash mid-write leaves either the old committed
version or nothing — never a half-written Semantic Object — and the legacy app's next `stat()`
reflects reality rather than a promise. A timed-out Query Resolver degrades to a partial,
clearly-labeled result set rather than blocking indefinitely, per Design Invariant 5 (degrade,
never fail closed on the user's goal).

## Performance Analysis

VirtualFolders are cached with a TTL and invalidated incrementally: rather than re-running a full
traversal on every access, the Query Resolver subscribes to relevant object/edge changes via
[31 — Event System](31-event-system.md) and only re-materializes the specific folders whose
inputs actually changed. Because path synthesis and ACL checks are the only per-request-unique
work once a VirtualFolder is cached, a legacy `ls`-equivalent call is expected to return in the
low tens of milliseconds for realistic personal-scale graphs, in line with the responsiveness
targets in [36 — Performance Benchmarks](36-performance-benchmarks.md). Embedding computation for
newly ingested objects is deferred to a background pipeline rather than performed inline on
write, so a large photo import does not block on semantic indexing before the bytes are durably
stored (see [28 — Storage Engine](28-storage-engine.md#algorithms)).

## Trade-offs

Presenting query results as dynamic, live-updating VirtualFolders is what makes "everything
related to my vacation" actually stay correct as new photos, emails, and receipts arrive — but it
is in tension with decades of POSIX applications that assume a directory tree only changes when
they change it. The `snapshot_token` mechanism (§Recovery) resolves this by trading strict
liveness for per-handle stability, which is the same trade-off traditional filesystems already
make for open file descriptors, just extended to directory handles. Keeping the graph as the only
source of truth, with no duplicated "compat filesystem" copy, avoids drift between what a native
query sees and what a legacy `ls` sees, at the cost of every legacy filesystem access paying for a
live query rather than a cheap directory-block read — mitigated, per §Performance Analysis, by
caching and incremental invalidation rather than by maintaining a second, independently-writable
tree. Finally, showing both a folder view and a semantic query bar simultaneously — rather than
picking one — is a deliberate concession to Universal Usability
(see [01 §5](01-vision-and-philosophy.md#5-universal-usability-highest-priority)): a user who
never wants to think in graphs can still drag files into folders and get durable, correct
behavior, without that choice limiting a power user who wants to query by meaning instead.

## Testing Strategy

The POSIX compat shim is validated against a standard POSIX filesystem compliance suite
(pjdfstest-class tests) to catch semantic drift from what legacy applications actually assume.
Property-based tests assert the folder-preservation invariant directly: for any sequence of
AI-driven re-organization, re-indexing, or GC events, every explicit `member_of` edge created by
a user-initiated `mkcollection`/`add_to_collection` call must still resolve after the sequence
completes. The natural-language-to-QuerySpec compiler is fuzzed with ambiguous and adversarial
phrasing to verify it degrades to a disambiguation prompt rather than a wrong silent guess.
Concurrency tests exercise simultaneous writes and live queries against the same region of the
graph to validate `snapshot_token` isolation. A golden end-to-end test encodes the vacation
scenario from this document's Motivation section directly: seed photos, hotel bookings, emails,
and calendar events per the fixture in
[29 — Database Schema](29-database-schema.md#worked-example-the-vacation-scenario), issue the
query, and assert every expected object type is present in the resulting VirtualFolder. These
feed the shared harness in [35 — Testing Strategy](35-testing-strategy.md).

---
*Next: [11 — Agent Runtime](11-agent-runtime.md).*
