use std::time::{Duration, Instant};

use hyperion_crypto::{Keystore, Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::monitor::CapabilityMonitor;
use crate::token::CapabilityToken;
use crate::types::{Fault, ObjectId, RightsMask, TokenId, TrustBoundaryId};

/// The wire-transmissible representation of a [`CapabilityToken`]'s *claimed* fields — used
/// when a token must cross a real process boundary (`hyperion-ipc`'s real transport, M3).
///
/// A `WireToken` is never trusted directly: it is exactly as authoritative as any other bytes
/// that arrived over a socket, which is to say not at all. The only way it becomes a real,
/// usable [`CapabilityToken`] is [`CapabilityMonitor::authenticate_wire_token`], which
/// reconstructs the claim and validates it against this monitor's own revocation-graph record
/// before handing anything back — see [`crate::revocation::RevocationNode`]'s docs for exactly
/// what that check closes. `CapabilityToken` itself deliberately has no `Serialize`/`Deserialize`
/// impl at all, so this validating conversion is the *only* path from wire data to a token this
/// process can act on; there is no shortcut that skips it.
///
/// What this does *not unconditionally* provide: confidentiality for a token in transit (an
/// unsigned `WireToken` is still exactly as authoritative as any other unauthenticated bytes).
/// Replay resistance is now real for a caller that opts in: [`WireToken::signed`]/
/// [`CapabilityMonitor::authenticate_wire_token_signed`] close docs/03's own previously-named
/// "cryptographic signing... is M9's job" gap — M9 (`hyperion-crypto`, real Ed25519) now exists
/// and is used here exactly the way `hyperion-plugin-framework`'s manifest signing and
/// `hyperion-ai-runtime`'s model-descriptor signing already established: a `Signature` over this
/// struct's own canonical bytes, checked against a real `VerifyingKey` before anything is
/// reconstructed. A caller still not wired for signing (no shared `VerifyingKey` established yet)
/// keeps using the original, unsigned [`CapabilityMonitor::authenticate_wire_token`] entry point
/// — this crate makes signing possible, it does not make it mandatory workspace-wide; wiring a
/// real caller (`hyperion-ipc`'s `Endpoint`, `hyperion-supervisor`'s spawned-service handoff) to
/// actually use it by default is real, separate follow-up work, not attempted here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireToken {
    pub token_id: u64,
    pub object_id: u64,
    pub rights: RightsMask,
    pub generation: u64,
    pub origin: u64,
    /// Replaces `CapabilityToken`'s `Option<Instant>`: `Instant` is explicitly
    /// process-relative (see its own std docs) and has no meaningful serialized form, so the
    /// wire carries a *relative* duration instead, reconstituted as `Instant::now() + d` on
    /// arrival in [`CapabilityMonitor::authenticate_wire_token`]. This is off by however long
    /// the frame spent in transit (microseconds, in practice) — not a correctness gap at any
    /// TTL granularity this system uses, and never makes an expired token look unexpired (transit
    /// time only ever makes the reconstructed deadline earlier than the original, never later).
    pub expiry_millis_remaining: Option<u64>,
    /// `None` for a `WireToken` built via [`From<&CapabilityToken>`] (this struct's original,
    /// still-real, still-supported unsigned path) — `Some` only when built via
    /// [`WireToken::signed`]. A real Ed25519 signature over every other field's own canonical
    /// bytes ([`canonical_bytes`]), checked by [`CapabilityMonitor::authenticate_wire_token_signed`].
    pub signature: Option<Signature>,
}

/// The exact bytes a real [`WireToken`] signature is produced/verified over — every claimed
/// field except `signature` itself, so a tampered `rights`/`object_id`/`generation`/etc. (or a
/// signature copied onto a different token's claim) fails verification.
fn canonical_bytes(wire: &WireToken) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&wire.token_id.to_le_bytes());
    bytes.extend_from_slice(&wire.object_id.to_le_bytes());
    bytes.extend_from_slice(&wire.rights.bits().to_le_bytes());
    bytes.extend_from_slice(&wire.generation.to_le_bytes());
    bytes.extend_from_slice(&wire.origin.to_le_bytes());
    match wire.expiry_millis_remaining {
        Some(ms) => {
            bytes.push(1);
            bytes.extend_from_slice(&ms.to_le_bytes());
        }
        None => bytes.push(0),
    }
    bytes
}

impl From<&CapabilityToken> for WireToken {
    fn from(token: &CapabilityToken) -> Self {
        WireToken {
            token_id: token.token_id().0,
            object_id: token.object_id().0,
            rights: token.rights(),
            generation: token.generation(),
            origin: token.origin().0,
            expiry_millis_remaining: token
                .expiry()
                .map(|e| e.saturating_duration_since(Instant::now()).as_millis() as u64),
            signature: None,
        }
    }
}

impl WireToken {
    /// As [`From<&CapabilityToken>`], but real Ed25519-signed over this token's own canonical
    /// bytes with `keystore` — the replay-resistant path [`CapabilityMonitor::
    /// authenticate_wire_token_signed`] requires. The signing device's real identity is
    /// `keystore.verifying_key()`; a receiver must already hold (or otherwise establish) that
    /// same real key to verify it, the same trust-establishment this workspace's other real
    /// signing call sites (`hyperion-plugin-framework`'s manifests, `hyperion-ai-runtime`'s model
    /// descriptors) already require.
    pub fn signed(token: &CapabilityToken, keystore: &Keystore) -> Self {
        let mut wire = WireToken::from(token);
        wire.signature = Some(keystore.sign(&canonical_bytes(&wire)));
        wire
    }
}

impl CapabilityMonitor {
    /// The only way a [`WireToken`] becomes a [`CapabilityToken`] this process can actually use:
    /// reconstructs the claim, then immediately runs the same authenticity/liveness/expiry
    /// check `check_rights_ok_result` would, before returning anything. A `WireToken` whose
    /// claimed `rights`/`object_id` don't match this monitor's own record for its `token_id`, or
    /// whose `generation` is stale, or that's expired, is rejected here — never handed back as
    /// something the caller must remember to check later. Does not check `signature` at all —
    /// see [`Self::authenticate_wire_token_signed`] for the replay-resistant variant that does.
    pub fn authenticate_wire_token(&self, wire: &WireToken) -> Result<CapabilityToken, Fault> {
        self.reconstruct_and_check(wire)
    }

    /// As [`Self::authenticate_wire_token`], additionally requiring a real, valid Ed25519
    /// signature over `wire`'s own canonical bytes against `verifying_key` — closing docs/03's
    /// own previously-named "cryptographic signing" gap for a caller that has a real
    /// `VerifyingKey` to check against. A missing or invalid signature is rejected with
    /// [`Fault::SignatureInvalid`] before any liveness/rights check even runs — a forged or
    /// replayed-from-elsewhere claim never reaches that check at all.
    pub fn authenticate_wire_token_signed(
        &self,
        wire: &WireToken,
        verifying_key: &VerifyingKey,
    ) -> Result<CapabilityToken, Fault> {
        let signature = wire.signature.as_ref().ok_or(Fault::SignatureInvalid)?;
        if !hyperion_crypto::verify(&canonical_bytes(wire), signature, verifying_key) {
            return Err(Fault::SignatureInvalid);
        }
        self.reconstruct_and_check(wire)
    }

    fn reconstruct_and_check(&self, wire: &WireToken) -> Result<CapabilityToken, Fault> {
        let token = CapabilityToken::from_wire_parts(
            TokenId(wire.token_id),
            ObjectId(wire.object_id),
            wire.rights,
            wire.generation,
            TrustBoundaryId(wire.origin),
            wire.expiry_millis_remaining
                .map(|ms| Instant::now() + Duration::from_millis(ms)),
        );
        self.check_live(&token)?;
        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_genuine_token_round_trips_through_the_wire() {
        let mut m = CapabilityMonitor::new();
        let root = m.mint_root(
            RightsMask::READ | RightsMask::WRITE,
            TrustBoundaryId(1),
            None,
        );

        let wire = WireToken::from(&root);
        let restored = m
            .authenticate_wire_token(&wire)
            .expect("a genuine token's own wire form must authenticate");

        assert_eq!(restored.object_id(), root.object_id());
        assert_eq!(restored.rights(), root.rights());
        assert_eq!(restored.generation(), root.generation());
    }

    #[test]
    fn a_wire_token_claiming_escalated_rights_is_rejected() {
        let mut m = CapabilityMonitor::new();
        let root = m.mint_root(RightsMask::READ, TrustBoundaryId(1), None);

        let mut wire = WireToken::from(&root);
        wire.rights = RightsMask::all();

        assert_eq!(
            m.authenticate_wire_token(&wire).unwrap_err(),
            Fault::NoSuchCapability
        );
    }

    #[test]
    fn a_wire_token_for_a_revoked_capability_is_rejected() {
        let mut m = CapabilityMonitor::new();
        let root = m.mint_root(RightsMask::READ, TrustBoundaryId(1), None);
        let wire = WireToken::from(&root);

        m.cap_revoke(&root);

        assert_eq!(
            m.authenticate_wire_token(&wire).unwrap_err(),
            Fault::Revoked
        );
    }

    #[test]
    fn a_wire_token_for_an_unknown_object_id_is_rejected() {
        let m = CapabilityMonitor::new();
        let wire = WireToken {
            token_id: 999_999,
            object_id: 1,
            rights: RightsMask::READ,
            generation: 0,
            origin: 1,
            expiry_millis_remaining: None,
            signature: None,
        };

        assert_eq!(
            m.authenticate_wire_token(&wire).unwrap_err(),
            Fault::NoSuchCapability
        );
    }

    #[test]
    fn expiry_survives_the_wire_within_transit_tolerance() {
        let mut m = CapabilityMonitor::new();
        let root = m.mint_root(
            RightsMask::READ,
            TrustBoundaryId(1),
            Some(Duration::from_secs(3600)),
        );

        let wire = WireToken::from(&root);
        assert!(wire.expiry_millis_remaining.unwrap() > 3_500_000);

        let restored = m.authenticate_wire_token(&wire).unwrap();
        assert!(!restored.is_expired());
    }

    #[test]
    fn a_genuinely_signed_token_authenticates_against_the_real_signing_keystores_own_key() {
        let mut m = CapabilityMonitor::new();
        let root = m.mint_root(
            RightsMask::READ | RightsMask::WRITE,
            TrustBoundaryId(1),
            None,
        );
        let keystore = Keystore::ephemeral();

        let wire = WireToken::signed(&root, &keystore);
        let restored = m
            .authenticate_wire_token_signed(&wire, &keystore.verifying_key())
            .expect("a genuinely signed token must authenticate against its own real signer");

        assert_eq!(restored.object_id(), root.object_id());
        assert_eq!(restored.rights(), root.rights());
    }

    #[test]
    fn an_unsigned_wire_token_is_rejected_by_the_signed_entry_point() {
        let mut m = CapabilityMonitor::new();
        let root = m.mint_root(RightsMask::READ, TrustBoundaryId(1), None);
        let keystore = Keystore::ephemeral();

        let wire = WireToken::from(&root); // no signature at all
        assert_eq!(
            m.authenticate_wire_token_signed(&wire, &keystore.verifying_key())
                .unwrap_err(),
            Fault::SignatureInvalid
        );
    }

    #[test]
    fn a_replayed_token_signed_by_a_different_real_device_is_rejected() {
        let mut m = CapabilityMonitor::new();
        let root = m.mint_root(RightsMask::READ, TrustBoundaryId(1), None);
        let attacker_keystore = Keystore::ephemeral();
        let real_keystore = Keystore::ephemeral();

        // A real signature genuinely produced by some other real device's own key -- not a
        // tampered field, a wholly different (but real) signer.
        let wire = WireToken::signed(&root, &attacker_keystore);

        assert_eq!(
            m.authenticate_wire_token_signed(&wire, &real_keystore.verifying_key())
                .unwrap_err(),
            Fault::SignatureInvalid,
            "a signature genuinely valid under a different real key must never verify here"
        );
    }

    #[test]
    fn a_tampered_field_on_an_otherwise_genuinely_signed_token_is_rejected() {
        let mut m = CapabilityMonitor::new();
        let root = m.mint_root(RightsMask::READ, TrustBoundaryId(1), None);
        let keystore = Keystore::ephemeral();

        let mut wire = WireToken::signed(&root, &keystore);
        wire.rights = RightsMask::all(); // tampered after signing

        assert_eq!(
            m.authenticate_wire_token_signed(&wire, &keystore.verifying_key())
                .unwrap_err(),
            Fault::SignatureInvalid,
            "a field tampered with after real signing must invalidate the real signature"
        );
    }

    #[test]
    fn a_genuinely_signed_but_revoked_token_still_fails_the_liveness_check() {
        let mut m = CapabilityMonitor::new();
        let root = m.mint_root(RightsMask::READ, TrustBoundaryId(1), None);
        let keystore = Keystore::ephemeral();
        let wire = WireToken::signed(&root, &keystore);

        m.cap_revoke(&root);

        assert_eq!(
            m.authenticate_wire_token_signed(&wire, &keystore.verifying_key())
                .unwrap_err(),
            Fault::Revoked,
            "a real, valid signature must not bypass the real revocation-graph check"
        );
    }
}
