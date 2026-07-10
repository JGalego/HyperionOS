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
//! never a mock.
//!
//! Deliberately deferred, matching this workspace's scoping convention (see
//! `hyperion-knowledge-graph`'s crate doc for the same pattern):
//!
//! - **Intent history, device/session state, calendar/comms signals**
//!   (docs/06 §Architecture's other four signal collectors) — these need
//!   [05 — Intent Engine](../05-intent-engine.md) (Phase 3) and consent-
//!   gated connectors ([16 — Privacy Architecture](../16-privacy-architecture.md),
//!   Phase 8) that don't exist yet. Only the Knowledge-Graph-backed working
//!   set and explicit-mention resolution are implemented.
//! - **Adaptive Complexity / `ExpertiseEstimate`** (docs/06 §5.4) is a read
//!   over vocabulary complexity and error-recovery behavior this crate has
//!   no source for yet (needs Phase 3's Intent Engine and Phase 4's Agent
//!   Runtime); [`engine::ContextEngine::current_expertise`] always returns a
//!   fixed, zero-confidence `Novice` estimate rather than fabricating a
//!   signal, and says so in its own `evidence` field.
//! - **Semantic summarization** (docs/06 §2's `summary` inclusion mode) —
//!   without Phase 3's Local AI Runtime, `summary` mode truncates metadata to
//!   its first few fields rather than computing a real summary. Noted at the
//!   call site in [`engine`].
//! - **Real signatures and trust-level classification** (docs/07 §5,
//!   §Algorithms 2) — [15 — Security Architecture](../15-security-architecture.md)'s
//!   `classify()` and real asymmetric signing don't exist until Phase 8.
//!   [`propagation::TrustLevel`] is caller-supplied rather than derived, and
//!   envelope "signing" is a non-cryptographic checksum — sufficient to
//!   prove and test the fail-closed-on-mismatch and no-replay behavior
//!   docs/07 requires, but explicitly not a security boundary yet.
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
mod propagation;
mod types;
mod working_set;

pub use engine::{ContextEngine, ContextError, EntityResolution, ExplainedEntry};
pub use propagation::{
    merge, ContextEnvelope, ContextPropagation, EnvelopeEntry, EnvelopeProvenance,
    EnvelopeStaleness, FreshnessReport, FreshnessStatus, Integrity, MergeOutcome, PropagationError,
    RedactionAction, RedactionPolicy, Representation, TrustLevel,
};
pub use types::{
    Budget, ContextBundle, ContextEntry, ExpertiseEstimate, ExpertiseLevel, InclusionMode, Scope,
};
