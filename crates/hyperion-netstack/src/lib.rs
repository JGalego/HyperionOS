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
//! [`workspace_bridge::present_disambiguation_as_workspace`] surfaces
//! docs/19 §10's human-in-the-loop disambiguation for real, compiling a
//! [`types::SemanticObjectRef`] flagged `needs_review` through the real
//! `hyperion-workspace` Phase 5 pipeline rather than leaving the flag
//! sitting unused on the node's metadata.
//!
//! docs/998-roadmap.md M10 adds real HTTP/TLS/DNS behind a new `real-http` Cargo feature
//! (off by default, same reason `hyperion-ai-runtime`'s `candle` feature is): [`fetch::ReqwestFetchBackend`]
//! is a real [`fetch::FetchBackend`] (real `reqwest` blocking client, real rustls TLS with a
//! *bundled* root store, real DNS) and [`extract::HtmlHeuristicExtractionBackend`] is a real
//! [`extract::ExtractionBackend`] (real `<title>`/`<meta name="description">`/`<p>` tag parsing
//! via `scraper`, no model in the loop). `MockFetchBackend`/`MockExtractionBackend` remain the
//! default for every existing test. See each real type's own doc comment for what real HTML/DOM
//! parsing and real transport-failure classification actually cover and don't.
//!
//! Deliberately deferred, and why:
//!
//! - ~~**The real HTTP/1.1/2/3, TLS 1.3, and DNS stack.**~~ — now real for HTTP/1.1 + TLS 1.3 +
//!   DNS (see the M10 note above); real HTTP/2/3(QUIC) specifically remain whatever `reqwest`'s
//!   own default negotiation provides, not something this crate configures or asserts on.
//! - ~~**Real HTML/DOM parsing** beyond [`extract::HtmlHeuristicExtractionBackend`]'s real but
//!   narrow `<title>`/`<meta name="description">`/`<p>` tag selectors~~ — real `schema.org`/
//!   JSON-LD/OpenGraph microformat parsing now exists: [`microformats::parse`] reads a real
//!   `<script type="application/ld+json">` block (preferred) or real `<meta property="og:*">`
//!   tags (fallback) and [`fetch::ReqwestFetchBackend`] populates
//!   [`types::FetchedPage::structured`] with it for real, rather than always `None`.
//!   ~~[`microformats`]'s own doc comment named one still-deferred piece: nested JSON-LD
//!   relationships~~ — now real too: every top-level property whose value is itself a real
//!   JSON-LD object (or array of objects) declaring its own `@type` (`author`/`publisher`, etc.)
//!   becomes a real [`types::StructuredSignal::relationships`] `(predicate, identifier)` pair,
//!   which [`hub::NetstackHub::web_research`]'s own pre-existing real edge-writing loop
//!   (previously starved of input for the JSON-LD path) now genuinely exercises against real
//!   fetched pages. `extract::HtmlHeuristicExtractionBackend`'s own `relationships: Vec::new()`
//!   remains exactly as scoped — a real HTML heuristic has no nested entity structure to walk.
//!   `MockFetchBackend` is unaffected — a fixture still declares `structured` directly.
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
//! - ~~**`robots.txt` fetching/parsing.**~~ — now real for [`fetch::ReqwestFetchBackend`]:
//!   [`robots::RobotsRules`] is a real parser (group selection by matched `User-agent`, falling
//!   back to `*`; longest-matching-prefix-wins between `Allow`/`Disallow`), and
//!   `ReqwestFetchBackend` performs a real `GET {scheme}://{host}/robots.txt` (cached per host for
//!   the lifetime of the backend) before ever fetching a disallowed path, setting
//!   [`types::FetchedPage::robots_disallowed`] for real rather than always `false`.
//!   `MockFetchBackend` is unaffected -- a fixture still declares the flag directly, exactly as
//!   before.
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
//! - **A real, model-driven disambiguation *decision*.**
//!   [`workspace_bridge::present_disambiguation_as_workspace`] now
//!   surfaces a `needs_review`-flagged [`types::SemanticObjectRef`]
//!   through a real, compiled `hyperion-workspace` Workspace (see this
//!   crate's "Real:" section above) — but nothing here decides *what the
//!   user chose*; this crate has no confirm/reject callback that would
//!   feed back into `resolve::find_match`'s `MatchDecision`, since that
//!   needs [13 — Dynamic UI Runtime](../13-dynamic-ui-runtime.md)'s own
//!   real input/binding plumbing this hosted simulator doesn't run.
//! - **`web.fetch.raw`'s real DOM/JS/cookie semantics for the
//!   [27 — Compatibility Layer](../27-compatibility-layer.md).**
//!   [`hub::NetstackHub::web_fetch_raw`] returns the mock backend's
//!   structured/unstructured payload verbatim with no Knowledge Graph
//!   merge; the Compatibility Layer itself is Phase 9.

mod canonical;
mod extract;
mod fetch;
mod hub;
#[cfg(feature = "real-http")]
mod microformats;
mod quarantine;
mod resolve;
#[cfg(feature = "real-http")]
mod robots;
mod types;
mod workspace_bridge;

#[cfg(feature = "real-http")]
pub use extract::HtmlHeuristicExtractionBackend;
pub use extract::{ExtractionBackend, MockExtractionBackend};
#[cfg(feature = "real-http")]
pub use fetch::ReqwestFetchBackend;
pub use fetch::{FetchBackend, FetchError, MockFetchBackend};
pub use hub::NetstackHub;
pub use types::{
    AuditEntry, CanonicalUrl, DomainEgressGrant, EntityType, ExtractedEntity, ExtractionMethod,
    FetchedPage, FreshnessPolicy, NetstackError, ObjectId, ResolutionCacheEntry, SemanticObjectRef,
    StructuredSignal, WebResolutionRequest,
};
pub use workspace_bridge::present_disambiguation_as_workspace;
