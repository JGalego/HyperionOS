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
//! Template matching itself (predicate/root-goal selection) is fully deterministic
//! (keyword/template matching, not a learned classifier), so exact-match testing is appropriate
//! for that path — see docs/35-testing-strategy.md's own note that the golden-path/
//! statistical-tolerance model applies once a real model backs parsing/decomposition, which is
//! now true of the one path named below that a real model backs: an unmatched utterance's
//! generated fallback plan. Exact-match testing there only asserts real, deterministic
//! properties of `MockBackend`'s own echo response (structure, non-emptiness, confidence
//! tier) — never a specific "reasonable-sounding" plan a real model would need statistical
//! tolerance to judge.
//!
//! Deliberately deferred, and why:
//!
//! - **Per-slot structured grounding** (docs/05 §4's `Slot`/`candidates`) —
//!   this crate grounds only a single implicit "target" entity reference
//!   per Intent (the mechanism the doc's own worked "the API" style
//!   examples need), not the full multi-slot model. Templates here declare
//!   no slots of their own.
//! - ~~**Generative decomposition**~~ (docs/05 §2's fallback for goal shapes with no matching HTN
//!   template) — now real: [`IntentEngine::new_with_plugins_and_ai_runtime`] wires a real
//!   `hyperion_ai_runtime::LocalAiRuntime`, and an utterance matching no curated or
//!   plugin-contributed template gets one real, model-generated ordered step list (each non-empty
//!   response line becomes a `TemplateLeaf` depending on the line before it) instead of always
//!   becoming a single, undecomposed root Intent. Confidence lands between a curated match's
//!   `0.9` and the old no-plan `0.3` — a real plan, but a model's own guess, not a hand-authored
//!   one. Still degrades honestly, never fabricates: no `ai_runtime` wired, an unauthorized
//!   token, no model resident for `ModelClass::Slm`, or a response with no usable lines all fall
//!   straight back to the original single-undecomposed-root behavior.
//! - **Only one built-in HTN template** (`docs/05`'s own worked "launch my
//!   startup" example, trimmed to four leaves) — proves the dependency-
//!   graph/priority/critical-path machinery works; a real system would
//!   maintain templates as versioned Semantic Objects (§2), which needs
//!   the authoring/editing surface this phase doesn't build. Partially closed
//!   (2026-07-16, docs/998-roadmap.md's Resourceful pillar):
//!   [`templates::match_template_with_plugins`] also matches a plugin's own real
//!   `hyperion_plugin_framework::Contribution::AutomationWorkflow` entries (via
//!   [`engine::IntentEngine::new_with_plugins`]), so a goal template no longer has to be one of
//!   this crate's own hardcoded built-ins to really decompose an utterance — still not the
//!   doc's own versioned-Semantic-Object authoring surface, just a second, real source of
//!   templates alongside it.
//! - **Conflict detection across active graphs** (docs/05 §6) — needs
//!   multiple concurrently-*executing* Intents with real Agents mid-
//!   execution ([12 — Multi-Agent Coordination](../12-multi-agent-coordination.md),
//!   Phase 4) for "exclusive-resource conflict" to mean anything real.
//! - **Using Working Memory as a real grounding signal.**
//!   [`engine::IntentEngine::handle_utterance`] now pushes every real
//!   utterance into a real `hyperion_memory::WorkingMemory` turn buffer
//!   per session ([`engine::IntentEngine::working_memory_turns`] exposes
//!   it) — docs/05 §7's `infer(slot, ctx, MemoryEngine.working_memory(...))`
//!   made real for the bookkeeping half. Actually *using* those turns to
//!   help resolve an ambiguous mention (rather than escalating to
//!   [`EntityResolution::Ambiguous`] immediately) needs real text/semantic
//!   matching this workspace's mock AI backend can't do yet — grounding
//!   itself still only reads `hyperion-context`.
//! - ~~`submit()` handing off to a real Multi-Agent Coordination~~ — now
//!   real: `hyperion-coordination::CoordinationSession::create_session`
//!   requires a real [`ExecutionTicket`] from [`IntentEngine::submit`] as
//!   its input, not a bare `NodeId` a caller could produce without ever
//!   calling `submit`.
//! - ~~Instrumenting decomposition with `hyperion-explainability`~~ — now
//!   real: [`engine::IntentEngine::handle_utterance`] opens a real
//!   Explanation Record around HTN decomposition, correlated by this
//!   engine's own real Intent `NodeId` — unlike `hyperion-coordination`/
//!   `hyperion-federation`'s stores, which record under a sentinel
//!   `triggering_intent_id` because neither owns a real Intent id source,
//!   this engine mints that id itself, so [`engine::IntentEngine::trace_intent`]
//!   is a genuine correlation. `hyperion-explainability`'s own doc named
//!   this crate's decomposition as one of its still-uninstrumented
//!   examples; that example is now closed (its own gap — retrofitting
//!   every other Phase 3-7 crate's decision points — remains open).
//!
//! Real (2026-07-16, docs/998-roadmap.md's Backlog "Protect the Human" item): the "no forced
//! 'think' checkpoint before intent decomposition" gap that item itself named as missing —
//! [`engine::IntentEngine::set_think_mode`] opts a session into a real, human-owned pause "before
//! Hyperion decides what a goal means," and [`engine::IntentEngine::handle_utterance`] really
//! withholds decomposition for that session (returning [`types::HandleOutcome::PendingThink`])
//! until an explicit [`engine::IntentEngine::proceed_with_decomposition`] call — never a default,
//! per that item's own explicit constraint. `hyperion-console`'s `/think on|off`/`/think-proceed`
//! meta-commands are the real, human-facing surface for it.

mod engine;
mod templates;
mod types;

pub use engine::{IntentEngine, IntentError};
pub use types::{ExecutionTicket, HandleOutcome, Intent, IntentStatus, MutationOp};
