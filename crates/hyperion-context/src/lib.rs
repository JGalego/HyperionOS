//! Hyperion L4 Context Engine + Propagation — Phase 2, third and final
//! slice.
//!
//! Implements docs/06-context-engine.md's Context Bundle assembly (signal
//! collection over [09 — Knowledge Graph](../09-knowledge-graph.md),
//! relevance ranking, budget-bounded inclusion, working-set hysteresis) and
//! docs/07-context-propagation.md's envelope contract (redaction by target
//! trust level, staleness/generation checking, signed-envelope integrity,
//! cross-boundary merge) — the two documents are one pipeline
//! (`assemble()` produces exactly what `export()` consumes) and are one
//! crate for the same reason `hyperion-knowledge-graph` merged docs/29 and
//! 09: splitting them would just duplicate `ContextEntry`/`EnvelopeEntry`
//! across a crate boundary.
//!
//! Phase 2's own exit criterion is narrow — "a Context Bundle can be
//! assembled for a synthetic Intent and correctly bounded in size" — so this
//! crate accepts a caller-constructed [`types::Scope`] standing in for a
//! real Intent (docs/41-implementation-phases.md: "still no Intent Engine,
//! Agents, or UI" in Phase 2). What *is* real: every signal this crate scores
//! against is read live from a real [`hyperion_knowledge_graph::KnowledgeGraph`],
//! never a mock — including, now, [`types::Scope::intent_id`] itself:
//! `hyperion-intent` persists every Intent as a real node in this same
//! graph, so [`engine::ContextEngine::assemble`] treats a `scope.intent_id`
//! that names one exactly like an explicit anchor (traversed for
//! neighbors too), docs/06 §Architecture's "Intent history" signal
//! collector made real rather than an inert label nothing ever read. An
//! `intent_id` that doesn't parse as a real node (any caller not yet
//! passing one) is silently ignored — no behavior change for them.
//!
//! Deliberately deferred, matching this workspace's scoping convention (see
//! `hyperion-knowledge-graph`'s crate doc for the same pattern):
//!
//! - **Calendar/comms signals** (docs/06 §Architecture's other signal
//!   collectors) — these need consent-gated connectors
//!   ([16 — Privacy Architecture](../16-privacy-architecture.md)) that
//!   don't exist yet. Device/session state, by contrast, is no longer
//!   blocked: `hyperion-device`'s `DeviceRegistry::register` now persists
//!   every `DeviceObject` as a real Knowledge Graph node, and
//!   `hyperion-device`'s own `tests/context_device_anchor.rs` proves that
//!   node composes as a real anchor here via the already-generic
//!   [`types::Scope::anchors`] — no dedicated `device_id`-as-anchor
//!   special case was needed the way [`types::Scope::intent_id`] required
//!   one, since `anchors` was never an inert field to begin with.
//! - ~~**Adaptive Complexity / `ExpertiseEstimate`** (docs/06 §5.4) is fully a
//!   read over vocabulary complexity, capability tier, and error-recovery
//!   behavior in docs/06's fuller design — sources this crate still cannot
//!   read from directly: `hyperion-intent` already depends on this crate (it
//!   passes a real `ContextEngine` into `IntentEngine::new`), so a reverse
//!   dependency back onto Intent Engine or Agent Runtime would be a real
//!   cycle, not just an unwired gap, even though both now exist (Phases 3/4).~~
//!   (2026-07-18) — now real, all three signals, without ever taking the reverse
//!   dependency: [`types::ExpertiseSignal`] is a narrowed local type this crate
//!   defines itself (the same "narrow the type, never take the reverse edge"
//!   precedent `hyperion-explainability`'s own `RecoveryPointId`/`SensitivityClass`
//!   and `hyperion-security`'s own `SensitivityHint` already established), and
//!   [`engine::ContextEngine::record_expertise_signal`] lets whichever real caller
//!   already depends on both sides push a real, already-computed sample in.
//!   `hyperion-intent::IntentEngine::handle_utterance` pushes a real
//!   [`expertise::vocabulary_complexity`] score for every real utterance (this
//!   crate's own scoring function, so both sides of that push agree on what
//!   "complex" means); `hyperion-console::ConsoleSession` pushes the other two —
//!   it already holds a real dispatch outcome and this session's own
//!   `ContextEngine` handle at once — mapping docs/06's own "raw API vs. guided
//!   workflow" onto whether a turn dispatched a single undecomposed Capability or
//!   a full HTN plan, and its own "self-corrects... or asks Hyperion to explain"
//!   onto the real `/redo` vs. `/teach` meta-commands.
//!   [`engine::ContextEngine::current_expertise`] blends all four real signals
//!   (working-set breadth/repetition plus whichever of the three pushed samples a
//!   session actually has) into one estimate, naming exactly which signals
//!   contributed in its own `evidence` field — a session with no activity or
//!   pushed samples yet still reports the same fixed, zero-confidence `Novice`
//!   estimate this method always returned, honestly labeled as such.
//! - ~~**Semantic summarization** (docs/06 §2's `summary` inclusion mode)~~ — now real:
//!   [`engine::ContextEngine::new_with_ai_runtime`] wires a real
//!   [`hyperion_ai_runtime::LocalAiRuntime`] in, and [`engine`]'s own `summarize` uses it to
//!   produce a real, model-generated summary of an entry's metadata rather than truncating it
//!   to the first few fields. Honest fallback, not a hard requirement: [`engine::ContextEngine::
//!   new`] (no `ai_runtime` wired), an unauthorized token, or nothing resident for
//!   `ModelClass::Slm` all fall back to the exact same truncation stand-in this bullet
//!   previously described as the *only* behavior — a caller loses summary fidelity, never the
//!   whole bundle.
//! - ~~Real signatures~~: now real. `hyperion-crypto` (Phase 8/M9) didn't
//!   exist when this bullet was written; [`propagation::ContextEnvelope`]'s
//!   `integrity.signature` is now a real Ed25519 [`hyperion_crypto::Signature`],
//!   produced by [`propagation::ContextPropagation::export`]'s caller-supplied
//!   `Keystore` and checked by [`propagation::ContextPropagation::import`]'s
//!   caller-supplied `VerifyingKey` — the same real signing/verifying split
//!   `hyperion-plugin-framework`/`hyperion-update` already established, not a
//!   checksum standing in for it anymore.
//! - **Trust-level classification** (docs/07 §5) — [15 — Security
//!   Architecture](../15-security-architecture.md)'s `classify()` still
//!   doesn't exist; [`propagation::TrustLevel`] stays caller-supplied
//!   rather than derived. A real, unforgeable signature and a real,
//!   *automatically derived* trust classification are different gaps —
//!   closing the first doesn't imply the second.
//! - **A production transport call site.** docs/07 §Interfaces says
//!   "Context Propagation owns only the envelope contract... not the
//!   bytes-on-the-wire transport" — this crate still has no built-in
//!   dependency on [30 — IPC Framework](../30-ipc-framework.md), and
//!   deliberately so (neither `hyperion-agent-runtime` nor
//!   `hyperion-federation` calls `export`/`import` today, so there is no
//!   real production call site to wire yet). What *is* now proven,
//!   dev-dependency-only, in `tests/ipc_transport.rs`: a real
//!   `ContextEnvelope` genuinely serializes to bytes, crosses a real
//!   `hyperion-ipc::IpcBus` `NOTIFY` frame between two separate Trust
//!   Boundaries, and imports cleanly on the other side — the envelope
//!   contract's shape survives a real wire hop, not just a same-call
//!   round trip.

mod engine;
mod expertise;
mod propagation;
mod types;
mod working_set;

pub use engine::{ContextEngine, ContextError, EntityResolution, ExplainedEntry};
pub use expertise::vocabulary_complexity;
pub use propagation::{
    merge, ContextEnvelope, ContextPropagation, EnvelopeEntry, EnvelopeProvenance,
    EnvelopeStaleness, FreshnessReport, FreshnessStatus, Integrity, MergeOutcome, PropagationError,
    RedactionAction, RedactionPolicy, Representation, TrustLevel,
};
pub use types::{
    Budget, CapabilityTierReach, ContextBundle, ContextEntry, ErrorRecoveryPattern,
    ExpertiseEstimate, ExpertiseLevel, ExpertiseSignal, InclusionMode, Scope,
};
