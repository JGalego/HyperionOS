use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::types::FetchedPage;

/// docs/19 §9's transport failure modes, reported by
/// [`FetchBackend::fetch`] instead of a real socket ever erroring — see
/// this crate's doc comment on the deferred real HTTP/TLS/DNS stack.
#[derive(Debug, Clone, thiserror::Error)]
pub enum FetchError {
    #[error("dns resolution failed for {0}")]
    Dns(String),
    #[error("tls handshake failed for {0}")]
    Tls(String),
    #[error("timed out fetching {0}")]
    Timeout(String),
    /// docs/998-roadmap.md M10: a real connection-establishment failure that a real
    /// backend's error classification could confirm was neither DNS nor TLS (e.g. TCP reset,
    /// network unreachable) -- empirically, a real closed port in this workspace's own dev
    /// sandbox doesn't even produce this shape (it hangs to a real client-side [`Self::Timeout`]
    /// instead of an instant refusal, confirmed by direct probing before writing this variant),
    /// but real hardware/networks can and do produce a real, fast refusal, and conflating that
    /// with [`Self::Timeout`] would be less honest than naming it.
    #[error("connection failed for {0}: {1}")]
    ConnectionFailed(String, String),
    #[error("no fixture registered for {0}")]
    NotFound(String),
}

/// docs/19 §3's "conventional requests" boundary, made swappable so this
/// crate never opens a real socket — the hosted-simulator convention this
/// whole workspace follows.
pub trait FetchBackend: Send + Sync {
    fn fetch(&self, canonical_url: &str) -> Result<FetchedPage, FetchError>;
}

#[derive(Clone)]
enum Fixture {
    Page(FetchedPage),
    Error(FetchError),
}

/// A deterministic stand-in for the real conventional network stack:
/// callers register exactly the [`FetchedPage`]s (or [`FetchError`]s) a
/// test needs by canonical URL. A lookup miss is a deterministic
/// [`FetchError::NotFound`], never real network I/O and never a panic —
/// every test byte reproducible.
#[derive(Default)]
pub struct MockFetchBackend {
    fixtures: Mutex<HashMap<String, Fixture>>,
}

impl MockFetchBackend {
    pub fn new() -> Self {
        MockFetchBackend {
            fixtures: Mutex::new(HashMap::new()),
        }
    }

    pub fn register(&self, canonical_url: impl Into<String>, page: FetchedPage) {
        self.fixtures
            .lock()
            .unwrap()
            .insert(canonical_url.into(), Fixture::Page(page));
    }

    /// Overrides (or pre-registers) `canonical_url` to fail — lets a test
    /// simulate an origin that goes down *after* an earlier successful
    /// fetch already populated the resolution cache, exercising docs/19
    /// §10's stale-but-labeled fallback.
    pub fn register_error(&self, canonical_url: impl Into<String>, error: FetchError) {
        self.fixtures
            .lock()
            .unwrap()
            .insert(canonical_url.into(), Fixture::Error(error));
    }
}

impl FetchBackend for MockFetchBackend {
    fn fetch(&self, canonical_url: &str) -> Result<FetchedPage, FetchError> {
        match self.fixtures.lock().unwrap().get(canonical_url) {
            Some(Fixture::Page(page)) => Ok(page.clone()),
            Some(Fixture::Error(e)) => Err(e.clone()),
            None => Err(FetchError::NotFound(canonical_url.to_string())),
        }
    }
}

/// Lets a caller keep an `Arc<MockFetchBackend>` to register fixtures on
/// after handing a `Box<dyn FetchBackend>` to [`crate::NetstackHub::new`]
/// — every test in this crate needs exactly this shared-ownership shape.
impl FetchBackend for Arc<MockFetchBackend> {
    fn fetch(&self, canonical_url: &str) -> Result<FetchedPage, FetchError> {
        self.as_ref().fetch(canonical_url)
    }
}

/// A real [`FetchBackend`] (docs/998-roadmap.md M10) -- a real HTTP client, real TLS
/// (rustls, with a *bundled* Mozilla root store rather than reading the OS's own trust store: the
/// actual target rootfs ships no `ca-certificates` package today, so a verifier reading a
/// nonexistent on-disk store would fail every real handshake on the booted image -- see this
/// crate's own Cargo.toml feature comment), and real DNS resolution, all via `reqwest`'s blocking
/// client -- the same synchronous-call-signature choice `hyperion-ai-runtime`'s M8 `CandleBackend`
/// already established via `hf-hub`'s blocking client, so this crate's existing sync
/// `FetchBackend` trait needs no async-runtime rework to host a real implementation.
#[cfg(feature = "real-http")]
pub struct ReqwestFetchBackend {
    client: reqwest::blocking::Client,
    /// A real, per-host `robots.txt` cache -- fetched (and parsed) at most once per host for the
    /// lifetime of this backend, not once per page fetch. Keyed by `"{scheme}://{host}"`, not the
    /// bare host, so a real `http://` and `https://` origin (genuinely distinct per the spec) are
    /// never conflated.
    robots_cache: std::sync::Mutex<std::collections::HashMap<String, crate::robots::RobotsRules>>,
}

#[cfg(feature = "real-http")]
impl ReqwestFetchBackend {
    /// A generous but bounded real timeout -- long enough for a real, slow origin, short enough
    /// that a real hung connection doesn't block this crate's caller indefinitely.
    const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

    /// Sent as this client's own real `User-Agent` header, and matched (case-insensitively)
    /// against a fetched `robots.txt`'s own `User-agent` groups by [`crate::robots::RobotsRules::
    /// parse`] -- a real crawler identifying itself honestly rather than fetching under a generic
    /// or absent identity.
    const USER_AGENT: &'static str = "hyperionos-netstack";

    pub fn new() -> Result<Self, FetchError> {
        Self::with_timeout(Self::TIMEOUT)
    }

    /// As [`Self::new`], with a caller-chosen timeout -- real production callers have no reason
    /// to need this (see [`Self::TIMEOUT`]'s own reasoning), but a real test proving a real
    /// timeout is detected shouldn't have to wait out the full production duration to prove it.
    pub fn with_timeout(timeout: std::time::Duration) -> Result<Self, FetchError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .user_agent(Self::USER_AGENT)
            // hub.rs's own redirect-following loop re-canonicalizes and re-checks SSRF at every
            // hop (docs/19's own real security requirement) -- if this client silently followed
            // redirects itself, every intermediate hop would bypass that per-hop check entirely.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| {
                FetchError::ConnectionFailed("<client init>".to_string(), e.to_string())
            })?;
        Ok(ReqwestFetchBackend {
            client,
            robots_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    /// `true` if `canonical_url` is allowed by its own real `robots.txt` -- fetched (a real `GET
    /// {scheme}://{host}/robots.txt`) and parsed at most once per host, then cached for this
    /// backend's own lifetime. A `robots.txt` that can't be reached at all (404, connection
    /// failure, timeout) allows everything, the same real convention as no `robots.txt` existing.
    fn robots_allows(&self, canonical_url: &str) -> bool {
        let Ok(url) = reqwest::Url::parse(canonical_url) else {
            return true;
        };
        let Some(host) = url.host_str() else {
            return true;
        };
        let origin = match url.port() {
            Some(port) => format!("{}://{host}:{port}", url.scheme()),
            None => format!("{}://{host}", url.scheme()),
        };

        if let Some(rules) = self.robots_cache.lock().unwrap().get(&origin) {
            return rules.allows(url.path());
        }

        let rules = match self.client.get(format!("{origin}/robots.txt")).send() {
            Ok(response) if response.status().is_success() => response
                .text()
                .map(|body| crate::robots::RobotsRules::parse(&body, Self::USER_AGENT))
                .unwrap_or_default(),
            _ => crate::robots::RobotsRules::default(),
        };
        let allowed = rules.allows(url.path());
        self.robots_cache.lock().unwrap().insert(origin, rules);
        allowed
    }

    /// Real error classification, based on this backend's own real, empirically-observed error
    /// shapes (probed directly against a real nonexistent domain, a real expired-certificate
    /// host, and a real slow endpoint before writing this function, not guessed from
    /// documentation): both a real DNS failure and a real TLS failure report
    /// `reqwest::Error::is_connect() == true`, distinguished only by their error source chain's
    /// message; a real timeout (including, in this workspace's own dev sandbox, a real closed
    /// port that never sends a real TCP reset) reports `is_timeout() == true` directly.
    fn classify_error(url: &str, err: &reqwest::Error) -> FetchError {
        use std::error::Error as _;

        if err.is_timeout() {
            return FetchError::Timeout(url.to_string());
        }
        if err.is_connect() {
            let mut cause = err.source();
            while let Some(source) = cause {
                let msg = source.to_string().to_lowercase();
                if msg.contains("dns") || msg.contains("lookup") || msg.contains("name or service")
                {
                    return FetchError::Dns(url.to_string());
                }
                if msg.contains("certificate")
                    || msg.contains("tls")
                    || msg.contains("invalid peer")
                {
                    return FetchError::Tls(url.to_string());
                }
                cause = source.source();
            }
            return FetchError::ConnectionFailed(url.to_string(), err.to_string());
        }
        FetchError::ConnectionFailed(url.to_string(), err.to_string())
    }
}

#[cfg(feature = "real-http")]
impl FetchBackend for ReqwestFetchBackend {
    fn fetch(&self, canonical_url: &str) -> Result<FetchedPage, FetchError> {
        // Checked, and honored, before ever requesting the page itself -- a real crawler must
        // not fetch a path its own robots.txt disallows, not merely label it disallowed after
        // fetching it anyway.
        if !self.robots_allows(canonical_url) {
            return Ok(FetchedPage {
                final_url: None,
                structured: None,
                text: String::new(),
                robots_disallowed: true,
                rate_limited: false,
            });
        }

        let response = self
            .client
            .get(canonical_url)
            .send()
            .map_err(|e| Self::classify_error(canonical_url, &e))?;

        let status = response.status();
        if status.is_redirection() {
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            return Ok(FetchedPage {
                // A malformed redirect with no real Location header re-visits the same
                // canonical form, which hub.rs's own loop already treats as "already visited" --
                // a safe, real bail-out rather than an infinite loop.
                final_url: Some(location.unwrap_or_else(|| canonical_url.to_string())),
                structured: None,
                text: String::new(),
                robots_disallowed: false,
                rate_limited: false,
            });
        }

        let rate_limited = status.as_u16() == 429;
        let text = response
            .text()
            .map_err(|e| Self::classify_error(canonical_url, &e))?;
        Ok(FetchedPage {
            final_url: None,
            // Real schema.org/JSON-LD/OpenGraph parsing remains this crate's own already-named
            // deferred gap -- this real backend always returns unstructured text, same as before.
            structured: None,
            text,
            // Always `false` here: a real `true` already returned above, before this real page
            // fetch ever ran.
            robots_disallowed: false,
            rate_limited,
        })
    }
}
