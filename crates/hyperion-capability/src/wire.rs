use std::time::{Duration, Instant};

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
/// What this does *not* provide: confidentiality or replay resistance for a token in transit.
/// If an attacker observes a real, valid `WireToken` on the wire (or the transport itself, e.g.
/// a shared filesystem socket, isn't itself access-controlled), they can replay those exact
/// bytes and be authenticated as the original holder, indistinguishably. Closing that requires
/// either transport-level access control (M2's Landlock/seccomp scoping of who can even reach a
/// given socket) or cryptographic signing — the latter is M9's job
/// ("real cryptography... a tampered plugin manifest/update package/audit-ledger entry is
/// rejected by a real signature"), not repeated here ahead of its own milestone. What *is* closed
/// here, unconditionally: nobody can claim rights or an object they were never granted for a
/// `token_id` they don't otherwise control, regardless of what bytes they send.
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
        }
    }
}

impl CapabilityMonitor {
    /// The only way a [`WireToken`] becomes a [`CapabilityToken`] this process can actually use:
    /// reconstructs the claim, then immediately runs the same authenticity/liveness/expiry
    /// check `check_rights_ok_result` would, before returning anything. A `WireToken` whose
    /// claimed `rights`/`object_id` don't match this monitor's own record for its `token_id`, or
    /// whose `generation` is stale, or that's expired, is rejected here — never handed back as
    /// something the caller must remember to check later.
    pub fn authenticate_wire_token(&self, wire: &WireToken) -> Result<CapabilityToken, Fault> {
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
}
