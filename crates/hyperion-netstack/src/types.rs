use serde::{Deserialize, Serialize};

/// A resolved Semantic Object's identity — reuses
/// [`hyperion_knowledge_graph::NodeId`] directly rather than a parallel
/// alias to `hyperion_storage::ObjectId`, since this crate never talks to
/// `hyperion-storage` itself.
pub type ObjectId = hyperion_knowledge_graph::NodeId;

/// docs/19 §4's `entity_type` closed set, kept as a Rust enum (unlike
/// `hyperion-knowledge-graph`'s open `ObjectType` string) since this
/// crate's own extraction/resolution/TTL logic branches on it by name and
/// docs/19 never mentions a plugin-defined web entity type the way docs/29
/// does for object types generally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityType {
    Paper,
    Person,
    Organization,
    Product,
    Community,
    Concept,
    Event,
    Place,
    /// docs/19 §9's "extraction produces no confident entity" fallback.
    WebPage,
}

impl EntityType {
    pub fn as_object_type(self) -> &'static str {
        match self {
            EntityType::Paper => "Paper",
            EntityType::Person => "Person",
            EntityType::Organization => "Organization",
            EntityType::Product => "Product",
            EntityType::Community => "Community",
            EntityType::Concept => "Concept",
            EntityType::Event => "Event",
            EntityType::Place => "Place",
            EntityType::WebPage => "WebPage",
        }
    }

    /// docs/19 §4's `ttl_class`: "a paper's metadata is near-immutable; a
    /// product's price is not." Concrete seconds are this crate's own
    /// hosted-simulator choice, not a value docs/19 pins down.
    pub(crate) fn default_ttl_secs(self) -> u64 {
        match self {
            EntityType::Paper | EntityType::Concept => 30 * 24 * 3600,
            EntityType::Organization | EntityType::Person | EntityType::Place => 7 * 24 * 3600,
            EntityType::Product | EntityType::Event => 6 * 3600,
            EntityType::Community | EntityType::WebPage => 24 * 3600,
        }
    }
}

/// docs/19 §4's `WebResolutionRequest`, narrowed to what
/// [`crate::NetstackHub::web_research`] actually consumes — `request_id`
/// is the caller's own concern (this crate hands back a
/// [`SemanticObjectRef`] per call, nothing keyed on a request id), and
/// `intent_id` is recorded into the audit trail via `purpose` rather than
/// as a separate typed field, since no Intent Engine reference is plumbed
/// through here.
#[derive(Debug, Clone)]
pub struct WebResolutionRequest {
    pub origin: String,
    pub agent_id: u64,
    pub purpose: String,
    pub freshness: FreshnessPolicy,
    /// Checked against the grant's `max_depth` as an authorization bound —
    /// see this crate's doc comment on deferred multi-hop crawling.
    pub depth: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessPolicy {
    UseCache,
    ForceRevalidate,
}

/// docs/19 §4's `CanonicalURL`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalUrl {
    pub raw_url: String,
    pub canonical_form: String,
    pub redirect_chain: Vec<String>,
    pub domain: String,
}

/// A signal the mock fetch backend hands back standing in for what a real
/// HTML parser would produce from `schema.org`/JSON-LD, OpenGraph, or an
/// identifier microformat — see this crate's doc comment on the deferred
/// real parser.
#[derive(Debug, Clone)]
pub struct StructuredSignal {
    pub entity_type: EntityType,
    pub identifier: Option<String>,
    pub fields: serde_json::Value,
    /// docs/19 §5.5: `(predicate, related_entity_identifier)` pairs
    /// extracted alongside the entity itself.
    pub relationships: Vec<(String, String)>,
}

/// docs/19 §7's `raw`/`sanitized` page, narrowed to what
/// [`crate::fetch::MockFetchBackend`] can supply deterministically: a
/// pre-parsed structured signal when the fixture represents a page with
/// one, unstructured fallback text otherwise, and an optional
/// `final_url` standing in for a followed redirect.
#[derive(Debug, Clone)]
pub struct FetchedPage {
    pub final_url: Option<String>,
    pub structured: Option<StructuredSignal>,
    pub text: String,
    pub robots_disallowed: bool,
    pub rate_limited: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractionMethod {
    StructuredData,
    ModelBased,
}

/// docs/19 §4's `ResolvedEntityCandidate`.
#[derive(Debug, Clone)]
pub struct ExtractedEntity {
    pub entity_type: EntityType,
    pub identifier: Option<String>,
    pub fields: serde_json::Value,
    pub confidence: f32,
    pub extraction_method: ExtractionMethod,
    pub relationships: Vec<(String, String)>,
}

/// docs/19 §4's `DomainEgressGrant`, keyed in
/// [`crate::hub::NetstackHub`] by [`hyperion_capability::CapabilityToken`]
/// identity (its `TokenId`) rather than a separate id this crate would
/// otherwise have to mint — one grant per capability delegation, which is
/// exactly docs/19 §8's "every `web.research` grant carries a
/// `DomainEgressGrant`."
#[derive(Debug, Clone)]
pub struct DomainEgressGrant {
    /// A pattern is either an exact domain (`"example.com"`) or a
    /// wildcard-subdomain pattern (`"*.example.com"`).
    pub domain_patterns: Vec<String>,
    pub rate_limit_per_window: u32,
    pub window_secs: u64,
    pub max_depth: u32,
    /// `None` means no expiry.
    pub expiry: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct GrantState {
    pub grant: DomainEgressGrant,
    pub calls_this_window: u32,
    pub window_started_at: u64,
}

/// docs/19 §4's `ResolutionCacheEntry`.
#[derive(Debug, Clone, Copy)]
pub struct ResolutionCacheEntry {
    pub object_id: ObjectId,
    pub content_fingerprint: u64,
    pub last_verified: u64,
    pub ttl_expiry: u64,
    pub entity_type: EntityType,
}

/// What [`crate::NetstackHub::web_research`] hands back per docs/19 §6's
/// `web.research` contract — `SemanticObjectRef` there, widened with the
/// `stale`/`needs_review` flags docs/19 §9-§10 treat as first-class
/// outcomes an Agent can act on, not buried inside a side channel.
#[derive(Debug, Clone, Copy)]
pub struct SemanticObjectRef {
    pub object_id: ObjectId,
    pub stale: bool,
    pub needs_review: bool,
}

/// docs/19 §8's full audit trail: "every fetch, merge, cache hit, and
/// quarantine event is logged with the requesting `agent_id`."
#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub timestamp: u64,
    pub agent_id: u64,
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, thiserror::Error)]
pub enum NetstackError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("no domain egress grant registered for this capability token")]
    NoGrant,
    #[error("grant has expired")]
    GrantExpired,
    #[error("domain '{0}' is not permitted by this grant")]
    DomainNotPermitted(String),
    #[error("requested depth {0} exceeds this grant's max_depth {1}")]
    DepthExceeded(u32, u32),
    #[error("rate limit exceeded for this grant")]
    RateLimited,
    #[error("refused: '{0}' resolves to a private/link-local address")]
    SsrfBlocked(String),
    #[error("circuit open for domain '{0}', failing fast")]
    CircuitOpen(String),
    #[error("fetch failed: {0}")]
    Fetch(#[from] crate::fetch::FetchError),
    #[error("knowledge graph error: {0}")]
    Graph(#[from] hyperion_knowledge_graph::GraphError),
}
