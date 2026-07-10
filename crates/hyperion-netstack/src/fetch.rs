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
