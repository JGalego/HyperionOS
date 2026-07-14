use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TokenId};
use hyperion_knowledge_graph::{EdgeOrigin, GraphQuery, KnowledgeGraph, NodeId};

use crate::canonical;
use crate::extract::{self, ExtractionBackend};
use crate::fetch::FetchBackend;
use crate::quarantine;
use crate::resolve::{self, MatchDecision};
use crate::types::{
    AuditEntry, CanonicalUrl, DomainEgressGrant, EntityType, ExtractedEntity, ExtractionMethod,
    FetchedPage, FreshnessPolicy, GrantState, NetstackError, ResolutionCacheEntry,
    SemanticObjectRef, WebResolutionRequest,
};

const RESOLVER_VERSION: &str = "hyperion-netstack/0.1";
/// docs/11-style circuit breaker: trips after N consecutive fetch
/// failures against one domain.
const CIRCUIT_THRESHOLD: u32 = 3;
const CIRCUIT_COOLDOWN_SECS: u64 = 30;
/// docs/19 §7's redirect-following loop needs a bound — a real browser
/// caps redirect chains too; this guards the mock backend against a
/// fixture that redirects to itself.
const MAX_REDIRECTS: usize = 8;

struct CircuitState {
    consecutive_failures: u32,
    opened_at: Option<u64>,
}

/// docs/998-roadmap.md M10: a bare `"*"` pattern matches any domain -- a real, deliberately
/// minimal addition for a general-purpose interactive caller (`hyperion-console`'s own real
/// undecomposed-goal fallback) that cannot pre-enumerate every real domain a user might ask
/// about. This does *not* weaken this crate's other real security checks, which all still apply
/// independently of which domain pattern matched: SSRF containment
/// (`canonical::is_private_or_local`) still runs regardless, and the grant's own
/// `rate_limit_per_window`/`max_depth`/`expiry` still bound abuse -- `"*"` removes only the
/// pre-approved-domain-allowlist restriction, not this crate's other real enforcement.
fn domain_matches(pattern: &str, domain: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    match pattern.strip_prefix("*.") {
        Some(suffix) => domain == suffix || domain.ends_with(&format!(".{suffix}")),
        None => domain == pattern,
    }
}

fn inferred_target_type(predicate: &str) -> EntityType {
    match predicate {
        "authored_by" | "employs" => EntityType::Person,
        "cites" => EntityType::Paper,
        "produces" => EntityType::Product,
        "member_of" => EntityType::Organization,
        _ => EntityType::WebPage,
    }
}

fn fingerprint(page: &FetchedPage) -> u64 {
    let mut hasher = DefaultHasher::new();
    match &page.structured {
        Some(s) => s.fields.to_string().hash(&mut hasher),
        None => page.text.hash(&mut hasher),
    }
    hasher.finish()
}

fn provenance(
    source_url: &str,
    method: ExtractionMethod,
    confidence: f32,
    now: u64,
) -> serde_json::Value {
    serde_json::json!({
        "source_url": source_url,
        "resolver_version": RESOLVER_VERSION,
        "extraction_method": match method {
            ExtractionMethod::StructuredData => "structured-data",
            ExtractionMethod::ModelBased => "model-based",
            ExtractionMethod::HtmlHeuristic => "html-heuristic",
        },
        "confidence": confidence,
        "timestamp": now,
    })
}

fn build_metadata(
    candidate: &ExtractedEntity,
    source_url: &str,
    now: u64,
    needs_review: bool,
) -> serde_json::Value {
    let mut fields = candidate.fields.clone();
    if let Some(obj) = fields.as_object_mut() {
        if let Some(id) = &candidate.identifier {
            obj.insert("identifier".to_string(), serde_json::json!(id));
        }
        obj.insert(
            "_provenance".to_string(),
            provenance(
                source_url,
                candidate.extraction_method,
                candidate.confidence,
                now,
            ),
        );
        if needs_review {
            obj.insert("needs_review".to_string(), serde_json::json!(true));
        }
    }
    fields
}

/// docs/19 — the Semantic Networking Layer. See this crate's doc comment
/// for the full real/deferred split.
pub struct NetstackHub {
    graph: Arc<KnowledgeGraph>,
    fetch_backend: Box<dyn FetchBackend>,
    extraction_backend: Box<dyn ExtractionBackend>,
    grants: Mutex<HashMap<TokenId, GrantState>>,
    cache: Mutex<HashMap<String, ResolutionCacheEntry>>,
    circuit_breakers: Mutex<HashMap<String, CircuitState>>,
    audit_log: Mutex<Vec<AuditEntry>>,
    quarantine_queue: Mutex<Vec<(String, String)>>,
}

impl NetstackHub {
    pub fn new(
        graph: Arc<KnowledgeGraph>,
        fetch_backend: Box<dyn FetchBackend>,
        extraction_backend: Box<dyn ExtractionBackend>,
    ) -> Self {
        NetstackHub {
            graph,
            fetch_backend,
            extraction_backend,
            grants: Mutex::new(HashMap::new()),
            cache: Mutex::new(HashMap::new()),
            circuit_breakers: Mutex::new(HashMap::new()),
            audit_log: Mutex::new(Vec::new()),
            quarantine_queue: Mutex::new(Vec::new()),
        }
    }

    fn require(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        rights: RightsMask,
    ) -> Result<(), NetstackError> {
        monitor
            .check_rights_ok_result(token, rights)
            .map_err(|_| NetstackError::Unauthorized)
    }

    fn audit(&self, agent_id: u64, now: u64, kind: &str, detail: &str) {
        self.audit_log.lock().unwrap().push(AuditEntry {
            timestamp: now,
            agent_id,
            kind: kind.to_string(),
            detail: detail.to_string(),
        });
    }

    pub fn audit_log(&self) -> Vec<AuditEntry> {
        self.audit_log.lock().unwrap().clone()
    }

    pub fn quarantine_queue(&self) -> Vec<(String, String)> {
        self.quarantine_queue.lock().unwrap().clone()
    }

    /// docs/19 §4/§8's `DomainEgressGrant`: an ordinary capability grant
    /// scoped to `target_token`'s identity, authorized by `admin_token`
    /// holding `WRITE` — mirrors how every other cross-boundary grant in
    /// this workspace is minted.
    pub fn grant_domain_egress(
        &self,
        monitor: &CapabilityMonitor,
        admin_token: &CapabilityToken,
        target_token: &CapabilityToken,
        grant: DomainEgressGrant,
        now: u64,
    ) -> Result<(), NetstackError> {
        self.require(monitor, admin_token, RightsMask::WRITE)?;
        self.grants.lock().unwrap().insert(
            target_token.token_id(),
            GrantState {
                grant,
                calls_this_window: 0,
                window_started_at: now,
            },
        );
        Ok(())
    }

    fn authorize_and_charge(
        &self,
        token: &CapabilityToken,
        domain: &str,
        depth: u32,
        now: u64,
    ) -> Result<(), NetstackError> {
        let mut grants = self.grants.lock().unwrap();
        let state = grants
            .get_mut(&token.token_id())
            .ok_or(NetstackError::NoGrant)?;

        if state.grant.expiry.is_some_and(|expiry| now > expiry) {
            return Err(NetstackError::GrantExpired);
        }
        if depth > state.grant.max_depth {
            return Err(NetstackError::DepthExceeded(depth, state.grant.max_depth));
        }
        if !state
            .grant
            .domain_patterns
            .iter()
            .any(|p| domain_matches(p, domain))
        {
            return Err(NetstackError::DomainNotPermitted(domain.to_string()));
        }
        if now.saturating_sub(state.window_started_at) >= state.grant.window_secs {
            state.window_started_at = now;
            state.calls_this_window = 0;
        }
        if state.calls_this_window >= state.grant.rate_limit_per_window {
            return Err(NetstackError::RateLimited);
        }
        state.calls_this_window += 1;
        Ok(())
    }

    fn circuit_open(&self, domain: &str, now: u64) -> bool {
        let mut breakers = self.circuit_breakers.lock().unwrap();
        let Some(state) = breakers.get_mut(domain) else {
            return false;
        };
        let Some(opened_at) = state.opened_at else {
            return false;
        };
        if now.saturating_sub(opened_at) >= CIRCUIT_COOLDOWN_SECS {
            state.opened_at = None;
            state.consecutive_failures = 0;
            false
        } else {
            true
        }
    }

    fn record_failure(&self, domain: &str, now: u64) {
        let mut breakers = self.circuit_breakers.lock().unwrap();
        let state = breakers.entry(domain.to_string()).or_insert(CircuitState {
            consecutive_failures: 0,
            opened_at: None,
        });
        state.consecutive_failures += 1;
        if state.consecutive_failures >= CIRCUIT_THRESHOLD {
            state.opened_at = Some(now);
        }
    }

    fn record_success(&self, domain: &str) {
        if let Some(state) = self.circuit_breakers.lock().unwrap().get_mut(domain) {
            state.consecutive_failures = 0;
            state.opened_at = None;
        }
    }

    fn cache_lookup(&self, canonical_form: &str) -> Option<ResolutionCacheEntry> {
        self.cache.lock().unwrap().get(canonical_form).copied()
    }

    fn create_stub_node(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        source_url: &str,
        note: &str,
        now: u64,
    ) -> Result<NodeId, NetstackError> {
        let metadata = serde_json::json!({
            "title": source_url,
            "note": note,
            "_provenance": provenance(source_url, ExtractionMethod::ModelBased, 0.0, now),
        });
        Ok(self.graph.put_node(
            monitor,
            token,
            None,
            EntityType::WebPage.as_object_type(),
            None,
            metadata,
        )?)
    }

    /// docs/19 §10's "stale-but-labeled fallback": prefer a cached (even
    /// expired) entity over nothing; only fabricate a stub when the cache
    /// has never seen this URL at all.
    fn degrade_to_stale_or_stub(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        canonical_form: &str,
        cache_hit: Option<ResolutionCacheEntry>,
        agent_id: u64,
        now: u64,
    ) -> Result<SemanticObjectRef, NetstackError> {
        if let Some(entry) = cache_hit {
            self.audit(agent_id, now, "stale_fallback", canonical_form);
            return Ok(SemanticObjectRef {
                object_id: entry.object_id,
                stale: true,
                needs_review: false,
            });
        }
        self.audit(agent_id, now, "stub_fallback", canonical_form);
        let stub_id =
            self.create_stub_node(monitor, token, canonical_form, "network fetch failed", now)?;
        Ok(SemanticObjectRef {
            object_id: stub_id,
            stale: false,
            needs_review: false,
        })
    }

    fn resolve_or_stub_related(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        predicate: &str,
        related_identifier: &str,
        source_url: &str,
        now: u64,
    ) -> Result<NodeId, NetstackError> {
        let entity_type = inferred_target_type(predicate);
        let query = GraphQuery {
            type_filter: Some(vec![entity_type.as_object_type().to_string()]),
            ..Default::default()
        };
        let hits = self.graph.query(monitor, token, &query)?;
        if let Some(hit) = hits.iter().find(|h| {
            h.node.metadata.get("identifier").and_then(|v| v.as_str()) == Some(related_identifier)
        }) {
            return Ok(hit.node_id);
        }
        let metadata = serde_json::json!({
            "identifier": related_identifier,
            "title": related_identifier,
            "_provenance": provenance(source_url, ExtractionMethod::StructuredData, 0.5, now),
        });
        Ok(self.graph.put_node(
            monitor,
            token,
            None,
            entity_type.as_object_type(),
            None,
            metadata,
        )?)
    }

    /// docs/19 §7's `web_research` pseudocode, end to end: authorize →
    /// canonicalize → cache lookup → fetch (following redirects,
    /// SSRF-checked at every hop) → quarantine scan → extract → resolve →
    /// merge → cache write → audit.
    pub fn web_research(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        request: &WebResolutionRequest,
        now: u64,
    ) -> Result<SemanticObjectRef, NetstackError> {
        self.require(monitor, token, RightsMask::EXEC)?;

        let canonical = canonical::canonicalize(&request.origin);
        self.authorize_and_charge(token, &canonical.domain, request.depth, now)?;

        if self.circuit_open(&canonical.domain, now) {
            self.audit(request.agent_id, now, "circuit_open", &canonical.domain);
            return Err(NetstackError::CircuitOpen(canonical.domain));
        }
        if canonical::is_private_or_local(&canonical.domain) {
            self.audit(request.agent_id, now, "ssrf_blocked", &canonical.domain);
            return Err(NetstackError::SsrfBlocked(canonical.domain));
        }

        let initial_cache_hit = self.cache_lookup(&canonical.canonical_form);
        if let Some(entry) = &initial_cache_hit {
            if request.freshness == FreshnessPolicy::UseCache && now <= entry.ttl_expiry {
                self.audit(
                    request.agent_id,
                    now,
                    "cache_hit",
                    &canonical.canonical_form,
                );
                return Ok(SemanticObjectRef {
                    object_id: entry.object_id,
                    stale: false,
                    needs_review: false,
                });
            }
        }

        let mut visited = HashSet::new();
        let mut current: CanonicalUrl = canonical.clone();
        let page = loop {
            if !visited.insert(current.canonical_form.clone())
                || current.redirect_chain.len() > MAX_REDIRECTS
            {
                self.record_failure(&current.domain, now);
                return self.degrade_to_stale_or_stub(
                    monitor,
                    token,
                    &canonical.canonical_form,
                    initial_cache_hit,
                    request.agent_id,
                    now,
                );
            }
            if canonical::is_private_or_local(&current.domain) {
                self.audit(request.agent_id, now, "ssrf_blocked", &current.domain);
                return Err(NetstackError::SsrfBlocked(current.domain));
            }
            match self.fetch_backend.fetch(&current.canonical_form) {
                Ok(p) if p.final_url.is_some() => {
                    let next = canonical::canonicalize(p.final_url.as_deref().unwrap());
                    let mut redirect_chain = current.redirect_chain.clone();
                    redirect_chain.push(current.canonical_form.clone());
                    current = CanonicalUrl {
                        raw_url: canonical.raw_url.clone(),
                        canonical_form: next.canonical_form,
                        redirect_chain,
                        domain: next.domain,
                    };
                }
                Ok(p) => break p,
                Err(e) => {
                    self.record_failure(&current.domain, now);
                    self.audit(
                        request.agent_id,
                        now,
                        "fetch_failed",
                        &format!("{}: {e}", current.canonical_form),
                    );
                    return self.degrade_to_stale_or_stub(
                        monitor,
                        token,
                        &canonical.canonical_form,
                        initial_cache_hit,
                        request.agent_id,
                        now,
                    );
                }
            }
        };
        self.record_success(&current.domain);
        let final_canonical_form = current.canonical_form;

        if !current.redirect_chain.is_empty() {
            if let Some(entry) = self.cache_lookup(&final_canonical_form) {
                if request.freshness == FreshnessPolicy::UseCache && now <= entry.ttl_expiry {
                    self.audit(
                        request.agent_id,
                        now,
                        "cache_hit_after_redirect",
                        &final_canonical_form,
                    );
                    return Ok(SemanticObjectRef {
                        object_id: entry.object_id,
                        stale: false,
                        needs_review: false,
                    });
                }
            }
        }

        if page.robots_disallowed || page.rate_limited {
            self.audit(
                request.agent_id,
                now,
                "skipped",
                &format!(
                    "{final_canonical_form}: robots_disallowed={}, rate_limited={}",
                    page.robots_disallowed, page.rate_limited
                ),
            );
            return self.degrade_to_stale_or_stub(
                monitor,
                token,
                &final_canonical_form,
                self.cache_lookup(&final_canonical_form),
                request.agent_id,
                now,
            );
        }

        let structured_fields = page.structured.as_ref().map(|s| &s.fields);
        let verdict = quarantine::scan(&page.text, structured_fields);
        if verdict.suspicious {
            let reason = verdict.reason.unwrap_or_default();
            self.audit(request.agent_id, now, "quarantined", &reason);
            self.quarantine_queue
                .lock()
                .unwrap()
                .push((final_canonical_form.clone(), reason));
            let stub_id = self.create_stub_node(
                monitor,
                token,
                &final_canonical_form,
                "content withheld pending review",
                now,
            )?;
            return Ok(SemanticObjectRef {
                object_id: stub_id,
                stale: false,
                needs_review: true,
            });
        }

        let candidate =
            extract::extract_entity(&page, self.extraction_backend.as_ref(), &request.purpose);
        let decision = resolve::find_match(&self.graph, monitor, token, &candidate)?;

        let (object_id, needs_review) = match decision {
            MatchDecision::ConfidentMatch(existing) => {
                let metadata = build_metadata(&candidate, &final_canonical_form, now, false);
                self.graph.put_node(
                    monitor,
                    token,
                    Some(existing),
                    candidate.entity_type.as_object_type(),
                    None,
                    metadata,
                )?;
                (existing, false)
            }
            MatchDecision::Ambiguous(_similar, _score) => {
                let metadata = build_metadata(&candidate, &final_canonical_form, now, true);
                let id = self.graph.put_node(
                    monitor,
                    token,
                    None,
                    candidate.entity_type.as_object_type(),
                    None,
                    metadata,
                )?;
                (id, true)
            }
            MatchDecision::Distinct => {
                let metadata = build_metadata(&candidate, &final_canonical_form, now, false);
                let id = self.graph.put_node(
                    monitor,
                    token,
                    None,
                    candidate.entity_type.as_object_type(),
                    None,
                    metadata,
                )?;
                (id, false)
            }
        };

        for (predicate, related_identifier) in &candidate.relationships {
            let related_id = self.resolve_or_stub_related(
                monitor,
                token,
                predicate,
                related_identifier,
                &final_canonical_form,
                now,
            )?;
            self.graph.link(
                monitor,
                token,
                object_id,
                predicate,
                related_id,
                1.0,
                EdgeOrigin::Explicit,
                Some(candidate.confidence),
                &final_canonical_form,
                None,
            )?;
        }

        self.cache.lock().unwrap().insert(
            final_canonical_form.clone(),
            ResolutionCacheEntry {
                object_id,
                content_fingerprint: fingerprint(&page),
                last_verified: now,
                ttl_expiry: now + candidate.entity_type.default_ttl_secs(),
                entity_type: candidate.entity_type,
            },
        );
        self.audit(request.agent_id, now, "resolved", &final_canonical_form);

        Ok(SemanticObjectRef {
            object_id,
            stale: false,
            needs_review,
        })
    }

    /// docs/19 §3.1/§6's `web.fetch.raw`: the same authorization and SSRF
    /// gate as `web.research`, but no Knowledge Graph merge — legacy
    /// [27 — Compatibility Layer](../27-compatibility-layer.md) content is
    /// never fed through the resolver by default.
    pub fn web_fetch_raw(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        url: &str,
        agent_id: u64,
        now: u64,
    ) -> Result<FetchedPage, NetstackError> {
        self.require(monitor, token, RightsMask::EXEC)?;

        let canonical = canonical::canonicalize(url);
        self.authorize_and_charge(token, &canonical.domain, 0, now)?;
        if canonical::is_private_or_local(&canonical.domain) {
            self.audit(agent_id, now, "ssrf_blocked", &canonical.domain);
            return Err(NetstackError::SsrfBlocked(canonical.domain));
        }

        let page = self.fetch_backend.fetch(&canonical.canonical_form)?;
        self.audit(agent_id, now, "fetch_raw", &canonical.canonical_form);
        Ok(page)
    }
}
