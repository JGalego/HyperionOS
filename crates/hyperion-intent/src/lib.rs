//! Hyperion L4 Intent Engine — Phase 3, fourth and final slice.
//!
//! Implements docs/05-intent-engine.md's core loop: parse an utterance into
//! a canonical predicate, decompose it against an HTN template into a
//! dependency-linked Intent Graph, resolve ambiguous entity references by
//! escalating rather than guessing (reusing
//! [`hyperion_context::ContextEngine::resolve_entity`] — grounding is a
//! shared concern, not reinvented here), derive priority from dependency
//! position and explicit urgency language, and reconcile a follow-up
//! utterance ("actually, cancel that") into a graph mutation rather than a
//! new Intent. Every Intent is a Semantic Object in a real
//! [`hyperion_knowledge_graph::KnowledgeGraph`] — `depends_on`/`informs`/
//! `supersedes` are real typed edges, not a parallel graph structure.
//!
//! This is fully deterministic (keyword/template matching, not a learned
//! classifier), so exact-match testing is appropriate for what's actually
//! implemented here — see docs/35-testing-strategy.md's own note that the
//! golden-path/statistical-tolerance model applies once a real model
//! backs parsing/decomposition, not before.
//!
//! Deliberately deferred, and why:
//!
//! - **Per-slot structured grounding** (docs/05 §4's `Slot`/`candidates`) —
//!   this crate grounds only a single implicit "target" entity reference
//!   per Intent (the mechanism the doc's own worked "the API" style
//!   examples need), not the full multi-slot model. Templates here declare
//!   no slots of their own.
//! - **Generative decomposition** (docs/05 §2's fallback for goal shapes
//!   with no matching HTN template) — needs a real planning model
//!   ([22 — Local AI Runtime](../22-local-ai-runtime.md)'s real backend).
//!   An utterance matching no template becomes a single, undecomposed root
//!   Intent (`status: proposed`) rather than a fabricated plan — degrade,
//!   never fail closed, but also never pretend.
//! - **Only one built-in HTN template** (`docs/05`'s own worked "launch my
//!   startup" example, trimmed to four leaves) — proves the dependency-
//!   graph/priority/critical-path machinery works; a real system would
//!   maintain templates as versioned Semantic Objects (§2), which needs
//!   the authoring/editing surface this phase doesn't build.
//! - **Conflict detection across active graphs** (docs/05 §6) — needs
//!   multiple concurrently-*executing* Intents with real Agents mid-
//!   execution ([12 — Multi-Agent Coordination](../12-multi-agent-coordination.md),
//!   Phase 4) for "exclusive-resource conflict" to mean anything real.
//! - **Memory Engine integration** (docs/05 §7's `infer(slot, ctx,
//!   MemoryEngine.working_memory(...))`, and Recovery Mechanisms' "a user
//!   correction is fed back into Memory Engine") — this crate's grounding
//!   only reads `hyperion-context`; wiring `hyperion-memory` in is
//!   straightforward once slot-level inference is real enough to need it.
//! - **`submit()` handing off to a real Multi-Agent Coordination** (Phase
//!   4, not built) — [`IntentEngine::submit`] returns an
//!   [`ExecutionTicket`] naming the ready-to-run leaves, but nothing
//!   actually assigns or dispatches an Agent to them yet.

mod engine;
mod templates;
mod types;

pub use engine::{IntentEngine, IntentError};
pub use types::{ExecutionTicket, HandleOutcome, Intent, IntentStatus, MutationOp};
