use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_crypto::{Keystore, VerifyingKey};

use crate::types::{ConsentGrant, DataScope, PrivacyError};

/// The exact bytes [`ConsentLedger::request`] signs and [`ConsentLedger::import`] verifies --
/// every field `ConsentGrant` has but `proof` itself, in a fixed order, mirroring
/// `hyperion_plugin_framework`'s own `canonical_bytes` convention for signing a struct that
/// carries its own signature field. Takes the fields directly rather than a whole `ConsentGrant`
/// so a caller never has to construct one with a placeholder `proof` just to compute this.
#[allow(clippy::too_many_arguments)]
fn canonical_bytes(
    id: u64,
    subject: u64,
    scope: &DataScope,
    purpose: &str,
    expiry: Option<u64>,
    granted_at: u64,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(&subject.to_le_bytes());
    match scope {
        DataScope::Domain(name) => {
            bytes.push(0);
            bytes.extend_from_slice(name.as_bytes());
        }
        DataScope::Object(node_id) => {
            bytes.push(1);
            bytes.extend_from_slice(&node_id.0.to_le_bytes());
        }
        DataScope::Capability(name) => {
            bytes.push(2);
            bytes.extend_from_slice(name.as_bytes());
        }
    }
    bytes.extend_from_slice(purpose.as_bytes());
    match expiry {
        Some(expiry) => {
            bytes.push(1);
            bytes.extend_from_slice(&expiry.to_le_bytes());
        }
        None => bytes.push(0),
    }
    bytes.extend_from_slice(&granted_at.to_le_bytes());
    bytes
}

fn grant_canonical_bytes(grant: &ConsentGrant) -> Vec<u8> {
    canonical_bytes(
        grant.id,
        grant.subject,
        &grant.scope,
        &grant.purpose,
        grant.expiry,
        grant.granted_at,
    )
}

/// `true` only if `grant.proof` is a genuine signature over exactly `grant`'s own canonical
/// bytes, produced by the private key matching `verifying_key` -- what [`ConsentLedger::import`]
/// checks before ever trusting a grant it didn't itself mint.
fn verify_grant(grant: &ConsentGrant, verifying_key: &VerifyingKey) -> bool {
    hyperion_crypto::verify(&grant_canonical_bytes(grant), &grant.proof, verifying_key)
}

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

    /// Mints a new grant and signs it with `device_key`'s own identity (`grant.proof`) -- see
    /// this module's own doc comment on why every grant this ledger ever issues carries a real,
    /// independently-verifiable signature, not just an opaque local record.
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
        device_key: &Keystore,
    ) -> Result<ConsentGrant, PrivacyError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let proof = device_key.sign(&canonical_bytes(id, subject, &scope, purpose, expiry, now));
        let grant = ConsentGrant {
            id,
            subject,
            scope,
            purpose: purpose.to_string(),
            expiry,
            granted_at: now,
            proof,
        };
        self.grants.lock().unwrap().insert(id, grant.clone());
        Ok(grant)
    }

    /// Accepts `grant` from another device (e.g. relayed over `hyperion-federation`'s own real,
    /// `SyncEnvelope`-authenticated transport) into this local ledger -- but only after verifying
    /// `grant.proof` against `verifying_key`, the issuing device's own public key. A grant whose
    /// signature doesn't verify (forged, corrupted, or signed by a device other than the one
    /// `verifying_key` names) is [`PrivacyError::SignatureInvalid`], never silently accepted --
    /// this is the real, independent check `grant.proof` exists for.
    pub fn import(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        grant: ConsentGrant,
        verifying_key: &VerifyingKey,
    ) -> Result<(), PrivacyError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        if !verify_grant(&grant, verifying_key) {
            return Err(PrivacyError::SignatureInvalid);
        }
        self.grants.lock().unwrap().insert(grant.id, grant);
        Ok(())
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
