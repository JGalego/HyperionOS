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
//! - **Real POSIX/FUSE mount.** `fs.mount_posix` doesn't exist; every call
//!   here is a direct Rust method, consistent with this workspace's
//!   hosted-simulator convention. [27 — Compatibility Layer](../27-compatibility-layer.md)
//!   (Phase 9) would be what actually mounts something.
//! - **Live VirtualFolder invalidation via a real Event System**
//!   ([31 — Event System](../31-event-system.md), not built). This crate's
//!   `snapshot_token` stability (docs/10 §Recovery Mechanisms) is achieved
//!   trivially and correctly for a different reason: a [`VirtualFolder`]'s
//!   member list is immutable once created, so every handle is already a
//!   frozen snapshot by construction — there is no live re-materialization
//!   to race against yet, so this is a real property, just not exercised
//!   under real background mutation.
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
mod types;
mod workspace_bridge;

pub use engine::{AnchorResolution, FsError, SemanticFilesystem};
pub use types::{Collection, DirEntry, QuerySpec, VirtualFolder};
pub use workspace_bridge::present_as_workspace;
