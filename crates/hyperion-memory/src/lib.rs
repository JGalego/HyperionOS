//! Hyperion L4 Memory Engine — Phase 3, second slice.
//!
//! Implements docs/08-memory-engine.md's four persisted tiers (Episodic,
//! Semantic, Procedural, Long-Term) plus an in-process Working Memory, its
//! decay-weighted (not TTL) consolidation scoring (§5.2), the multi-stage
//! decay funnel that changes *prominence* never *existence* (§5.3), and the
//! frequency-gated Episodic → Semantic/Procedural extraction (§5.4). Every
//! persisted [`MemoryRecord`] is a Semantic Object in a real
//! [`hyperion_knowledge_graph::KnowledgeGraph`] — docs/08 §3 is explicit
//! that this is what lets a memory record participate in graph traversal —
//! never a second, parallel store; this crate's `MemoryEngine` is a typed
//! view over `hyperion-knowledge-graph` nodes, not a new persistence layer.
//! [`engine::MemoryEngine::run_co_occurrence_pass`] is docs/09 §5.2's
//! inferred-edge background job's `co-occurs-with` half, made real: a
//! real `hyperion-scheduler` `BatchDistributable` task links every pair
//! of Knowledge Graph objects a real `MemoryRecord.provenance` names
//! together — `hyperion-knowledge-graph`'s own doc comment named exactly
//! this pairing (a real Memory Engine plus a real scheduler-driven job)
//! as the missing piece.
//!
//! Deliberately deferred, and why:
//!
//! - **Model-estimated salience** (docs/08 §5.2's `I(r) = max(explicit_flag,
//!   model_estimated_salience)`) — needs a real model; `I(r)` here is just
//!   the caller-supplied `importance` flag. Wire in
//!   `hyperion-ai-runtime` once a real backend can estimate salience.
//! - **Embedding-similarity clustering for extraction** (§5.4's "cluster
//!   recent unconsolidated episodes by shared entities and embedding
//!   similarity") — this crate groups by an explicit, caller-supplied
//!   `entity_key` string on Episodic content instead of real embedding
//!   clustering, which needs [22 — Local AI Runtime](../22-local-ai-runtime.md)'s
//!   real backend to produce meaningful embeddings to cluster over. The
//!   frequency gate itself (≥3 independent occurrences before promotion) is
//!   real and tested.
//! - **Working → Episodic distillation via a local model** (§5.1) — this
//!   crate accepts a caller-supplied summary rather than summarizing the
//!   turn buffer itself; wire in `hyperion-ai-runtime` once summarization
//!   is a real capability.
//! - **Cold-tier storage migration** (§5.3's "moved to cold storage in
//!   [28 — Storage Engine]") — this crate has one tier of physical storage
//!   (whatever `hyperion-knowledge-graph` uses); "dormant" is a metadata
//!   flag that changes default-query visibility, not a physical migration.
//! - **CryptoShred erasure mode / multi-device sync** ([16 — Privacy
//!   Architecture](../16-privacy-architecture.md), Phase 8/7) — `erase`
//!   here is SoftDelete only (an `erased` flag, filtered from query by
//!   default, never physically removed) — no crypto-shred, no cross-device
//!   propagation.
//! - **Per-tier/per-sensitivity capability scoping** (docs/08 §8's
//!   "third-party Capabilities... must be granted a scoped capability
//!   token per tier and per sensitivity level") — every call here still
//!   uses the same coarse READ/WRITE rights check every other crate in this
//!   workspace uses; sensitivity-tiered access is Phase 8's job
//!   ([15 — Security Architecture](../15-security-architecture.md)).
//!
//! Real (2026-07-16, docs/998-roadmap.md's Resourceful pillar): [`providers::capability_for`]/
//! [`providers::capabilities_for`] are the real `(tier, entity_key) -> capability_id` registry
//! docs/24's own "memory providers register storage backends into [08 — Memory Engine]" gap
//! named as missing — a plugin's `hyperion_plugin_framework::Contribution::MemoryProvider`
//! declares which capability can supply facts about an entity this crate has no local record of,
//! the same honest, never-bypass-dispatch shape `hyperion-knowledge-graph`'s own
//! `KnowledgeProvider` lookup already established.

mod decay;
mod engine;
mod providers;
mod types;

pub use decay::{decay_score, THETA_ARCHIVE, THETA_PROMOTE};
pub use engine::{ErasureReceipt, ExtractionReceipt, MemoryEngine, MemoryError, MemoryFilter};
pub use providers::{capabilities_for, capability_for};
pub use types::{MemoryRecord, MemoryTier, WorkingMemory};
