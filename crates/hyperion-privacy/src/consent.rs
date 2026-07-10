use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};

use crate::types::{ConsentGrant, DataScope, PrivacyError};

/// docs/16 §6's `privacy.consent.*` — "never assume consent": absence
/// (never-granted or revoked) always resolves to no standing grant, never
/// a default allow.
pub struct ConsentLedger {
    grants: Mutex<HashMap<u64, ConsentGrant>>,
    next_id: AtomicU64,
}

impl Default for ConsentLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsentLedger {
    pub fn new() -> Self {
        ConsentLedger {
            grants: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    fn require(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        rights: RightsMask,
    ) -> Result<(), PrivacyError> {
        monitor
            .check_rights_ok_result(token, rights)
            .map_err(|_| PrivacyError::Unauthorized)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn request(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        subject: u64,
        scope: DataScope,
        purpose: &str,
        expiry: Option<u64>,
        now: u64,
    ) -> Result<ConsentGrant, PrivacyError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let grant = ConsentGrant {
            id,
            subject,
            scope,
            purpose: purpose.to_string(),
            expiry,
            granted_at: now,
        };
        self.grants.lock().unwrap().insert(id, grant.clone());
        Ok(grant)
    }

    pub fn revoke(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        grant_id: u64,
    ) -> Result<(), PrivacyError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        self.grants.lock().unwrap().remove(&grant_id);
        Ok(())
    }

    /// docs/16 §5's `consent_ledger.standing_grant(capability)` — the one
    /// lookup [`crate::routing::route_capability_call`] trusts. Returns
    /// the live grant matching `subject`+`scope` if one exists, `None`
    /// otherwise (revoked or never granted are indistinguishable to a
    /// caller — both mean "no standing consent," by design).
    pub fn standing_grant(
        &self,
        subject: u64,
        scope: &DataScope,
        now: u64,
    ) -> Option<ConsentGrant> {
        self.grants
            .lock()
            .unwrap()
            .values()
            .find(|g| g.subject == subject && &g.scope == scope && g.is_live(now))
            .cloned()
    }

    pub fn list(&self, subject: Option<u64>) -> Vec<ConsentGrant> {
        self.grants
            .lock()
            .unwrap()
            .values()
            .filter(|g| subject.is_none_or(|s| g.subject == s))
            .cloned()
            .collect()
    }
}
