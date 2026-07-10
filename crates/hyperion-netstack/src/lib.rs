//! Hyperion L2 Platform Services — Semantic Networking Layer, Phase 7
//! third slice.
//!
//! Implements docs/19-networking-stack.md's `web.research` pipeline —
//! canonicalize → cache lookup → fetch → quarantine-scan → extract →
//! resolve-against-the-Knowledge-Graph → merge → cache write — as one
//! honest, capability-gated slice on top of the already-real
//! [`hyperion_knowledge_graph::KnowledgeGraph`], plus `web.fetch.raw`'s
//! parallel lower-trust lane (§3.1) that never merges into the graph.
//!
//! Real: URL canonicalization (tracking-parameter stripping, scheme/host
//! normalization, redirect-chain following bounded and re-checked for
//! SSRF at every hop); a resolution cache with per-entity-type TTL classes
//! and stale-but-labeled fallback (§10); a per-domain circuit breaker;
//! `DomainEgressGrant`s scoped to a specific `CapabilityToken` identity
//! (domain patterns, rate limit, max traversal depth, expiry), enforced
//! at this crate's boundary and never trusted to the caller; SSRF
//! containment by syntactic private/link-local address matching; a
//! prompt-injection quarantine scanner that withholds suspicious content
//! from the graph merge rather than discarding it (§10's
//! quarantine-and-review); structured-signal-first entity extraction with
//! a deterministic model-based fallback; and entity resolution against
//! the Knowledge Graph via exact external identifier match or a
//! token-overlap similarity proxy (see §5.4 below), writing typed
//! relationship edges alongside the resolved entity in the same call.
//!
//! Deliberately deferred, and why:
//!
//! - **The real HTTP/1.1/2/3, TLS 1.3, and DNS stack.** Per this
//!   workspace's hosted-simulator convention, [`fetch::FetchBackend`] is a
//!   trait with a deterministic [`fetch::MockFetchBackend`] — no socket is
//!   ever opened. Real transport failure *shapes* (DNS/TLS/timeout) are
//!   modeled as [`fetch::FetchError`] variants a fixture can return.
//! - **Real HTML/DOM parsing.** [`types::FetchedPage`] carries an already-
//!   parsed [`types::StructuredSignal`] (standing in for a real
//!   `schema.org`/JSON-LD/OpenGraph/microformat extractor) or raw
//!   unstructured text — this crate never tokenizes HTML.
//! - **Real embeddings for entity-resolution similarity (§5.4).** No
//!   embedding producer exists in this pipeline (Phase 3's Local AI
//!   Runtime embeddings were never wired into web extraction). A
//!   normalized title/name token-overlap ratio stands in for "high-
//!   confidence embedding similarity" — see [`resolve`]'s doc comment;
//!   this is a materially cruder proxy than this workspace's usual "pass
//!   a pre-computed `Vec<f32>`" deferral, called out rather than dressed
//!   up as equivalent.
//! - **A real local-model extraction pass.** [`extract::ExtractionBackend`]
//!   is a trait; [`extract::MockExtractionBackend`] deterministically
//!   derives a low-confidence generic `WebPage` from the fetched text,
//!   reaching docs/19 §9's own "no confident entity → generic WebPage"
//!   outcome without a model in the loop. Routing through
//!   [22 — Local AI Runtime](../22-local-ai-runtime.md)/
//!   [23 — Multi-Model Orchestration](../23-multi-model-orchestration.md)
//!   is a real integration this crate's trait boundary leaves open but
//!   does not perform.
//! - **`robots.txt` fetching/parsing.** [`types::FetchedPage`] carries a
//!   `robots_disallowed` flag a fixture declares directly; this crate
//!   does not fetch or parse a real `robots.txt`.
//! - **Real prompt-injection classification.** [`quarantine`] is a fixed
//!   denylist substring scanner, not a model-based classifier.
//! - **Multi-hop citation-following crawls (§4's `depth`).** `depth` is
//!   checked against the grant's `max_depth` as an authorization bound
//!   only (§8's security contract); this crate performs exactly one
//!   entity resolution per `web.research` call, never a recursive crawl.
//! - **The Event System's `web.entity.resolved` publish** ([31 — Event
//!   System](../31-event-system.md)) — no Event System crate exists yet
//!   in this workspace; the hook point is named in docs/19 §6 but not
//!   wired here.
//! - **Human-in-the-loop disambiguation UI.** `needs_review` is recorded
//!   on the created node's metadata (§10); surfacing it through an active
//!   Workspace is [13 — Dynamic UI Runtime](../13-dynamic-ui-runtime.md)'s
//!   concern and not wired into this crate.
//! - **`web.fetch.raw`'s real DOM/JS/cookie semantics for the
//!   [27 — Compatibility Layer](../27-compatibility-layer.md).**
//!   [`hub::NetstackHub::web_fetch_raw`] returns the mock backend's
//!   structured/unstructured payload verbatim with no Knowledge Graph
//!   merge; the Compatibility Layer itself is Phase 9.

mod canonical;
mod extract;
mod fetch;
mod hub;
mod quarantine;
mod resolve;
mod types;

pub use extract::{ExtractionBackend, MockExtractionBackend};
pub use fetch::{FetchBackend, FetchError, MockFetchBackend};
pub use hub::NetstackHub;
pub use types::{
    AuditEntry, CanonicalUrl, DomainEgressGrant, EntityType, ExtractedEntity, ExtractionMethod,
    FetchedPage, FreshnessPolicy, NetstackError, ObjectId, ResolutionCacheEntry, SemanticObjectRef,
    StructuredSignal, WebResolutionRequest,
};
