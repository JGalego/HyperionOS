//! Hyperion L4/L6 Semantic Filesystem — Phase 6.
//!
//! Implements docs/10-semantic-filesystem.md's "query-as-navigation" view
//! layer over a real [`hyperion_knowledge_graph::KnowledgeGraph`]: the same
//! query resolves identically whether it arrives as a structured
//! [`QuerySpec`] from a native caller or a synthesized POSIX-style path —
//! there is exactly one resolution path, not two, matching the doc's own
//! architecture. This crate owns neither the graph model (09) nor the
//! physical bytes (28) — it owns the translation between "a query" and "a
//! folder," and between "a legacy path" and a Semantic Object.
//!
//! Also covers this phase's other two mandate items: **universal search**
//! ([`resolve_query_from_mention`] turns "everything related to my
//! vacation" into a [`QuerySpec`] by reusing
//! [`hyperion_context::ContextEngine::resolve_entity`] for anchor
//! resolution — the same grounding mechanism `hyperion-intent` already
//! uses, not a second one) and **workspace generation**
//! ([`present_as_workspace`] turns a [`VirtualFolder`] into a real
//! `hyperion_workspace::WorkspaceGraph` by wrapping its members in a
//! synthetic Context Bundle and compiling it through the real Phase 5
//! compiler).
//!
//! Real: bounded-hop relational traversal merged and deduplicated with
//! vector-similarity results (docs/10 §Algorithms' "query resolution");
//! deterministic path synthesis with stable, object-id-keyed collision
//! disambiguation (§Algorithms' "path synthesis," §Recovery Mechanisms);
//! the Collection-as-ordinary-Semantic-Object mechanism (§Data Structures)
//! — a user-created folder is a real Knowledge Graph node plus real
//! explicit `member_of` edges, so it survives exactly as durably as any
//! other explicit fact; and the write-back distinction between landing in
//! a real Collection (fabricates an explicit edge) versus a virtual,
//! query-materialized folder (pins the path without inventing a false
//! edge) — docs/10's central "no silent authority" guarantee.
//!
//! Deliberately deferred, and why:
//!
//! - ~~**Real POSIX/FUSE mount.**~~ — now real: [`posix::mount_posix`]/[`posix::spawn_mount_posix`]
//!   mount a real `fuser::Filesystem` adapter (pure-Rust mount path, no system libfuse headers
//!   needed — see that module's own doc comment) over this same [`SemanticFilesystem`], resolving
//!   every VFS call through its already-real, capability-gated methods rather than a second data
//!   model. See [`posix`]'s own doc comment for the real, honestly-named scope this operates
//!   within (one fixed capability identity per mount; no `unlink`/`rmdir` yet, since this crate
//!   itself exposes no delete operation to translate to).
//! - ~~**Live VirtualFolder invalidation via a real Event System**~~ — now real:
//!   [`SemanticFilesystem::with_events`] wires a real `hyperion-events::EventBus` (subscribing to
//!   `hyperion_knowledge_graph::KnowledgeGraph::with_events`'s own `ObjectChanged` publications),
//!   and [`SemanticFilesystem::query`] now reuses a cached, still-fresh [`VirtualFolder`] for a
//!   repeat query of the same structural shape instead of always re-materializing — the real
//!   caching docs/10 §Performance Analysis always described. A cached folder is evicted the
//!   moment a write touches its anchor or any of its own members, so the next `query()` call for
//!   that shape re-materializes fresh rather than serving stale membership until TTL expiry. A
//!   [`VirtualFolder`]'s *own* member list is still immutable once materialized (docs/10
//!   §Recovery Mechanisms' `snapshot_token` stability), exactly as before — invalidation discards
//!   a stale folder and its cache entry, it never mutates one in place.
//! - **Per-object ACL re-check at materialization** (docs/10 §Security
//!   Considerations) — this crate applies the same coarse capability-
//!   rights check every call into `hyperion-knowledge-graph` already
//!   performs; finer per-object ACL enforcement is deferred to Phase 8
//!   exactly as `hyperion-knowledge-graph`'s own crate doc defers it — no
//!   second permission system is introduced here either.
//! - **pjdfstest-class POSIX compliance testing** — there is no real
//!   filesystem surface to run it against.

mod engine;
mod path;
pub mod posix;
mod types;
mod workspace_bridge;

pub use engine::{AnchorResolution, FsError, SemanticFilesystem};
pub use types::{Collection, DirEntry, QuerySpec, VirtualFolder};
pub use workspace_bridge::present_as_workspace;
