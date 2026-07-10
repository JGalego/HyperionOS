use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::revocation::{RevocationGraph, RevocationReceipt};
use crate::table::{CapabilityTable, SlotIndex};
use crate::token::CapabilityToken;
use crate::types::{min_expiry, Fault, ObjectId, RightsMask, TokenId, TrustBoundaryId};

/// The Capability Monitor: the only routine that mints, derives, revokes, or
/// validates a [`CapabilityToken`], per docs/03-kernel-architecture.md's
/// "Capability Security as the Kernel Primitive" — in the hosted simulator
/// this plays the role the privileged core plays on real hardware.
#[derive(Debug, Default)]
pub struct CapabilityMonitor {
    graph: RevocationGraph,
    next_object_id: AtomicU64,
}

impl CapabilityMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocates a fresh, monitor-assigned object identity — the simulator's
    /// stand-in for `device_claim` / `sandbox_create` minting a new kernel
    /// object (docs/03 §Interfaces / APIs).
    pub fn new_object(&self) -> ObjectId {
        ObjectId(self.next_object_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Mints a fresh root capability over a newly allocated object. Every
    /// other token for that object is reached by attenuating this one.
    pub fn mint_root(
        &mut self,
        rights: RightsMask,
        origin: TrustBoundaryId,
        ttl: Option<Duration>,
    ) -> CapabilityToken {
        let object_id = self.new_object();
        let token_id = TokenId::next();
        self.graph.insert_root(token_id);
        CapabilityToken {
            token_id,
            object_id,
            rights,
            generation: 0,
            origin,
            expiry: ttl.map(|d| Instant::now() + d),
        }
    }

    /// `cap_derive` — docs/03-kernel-architecture.md §Algorithms /
    /// §Pseudocode. Delegation with attenuation: never copies `parent`, mints
    /// a strictly-narrower child and attaches it beneath `parent` in the
    /// revocation graph. Rejects any attempt to widen rights or outlive the
    /// parent's own expiry ("attenuation only: subset, never superset").
    pub fn cap_derive(
        &mut self,
        parent: &CapabilityToken,
        rights: RightsMask,
        ttl: Option<Duration>,
        new_origin: TrustBoundaryId,
    ) -> Result<CapabilityToken, Fault> {
        self.check_live(parent)?;
        if !parent.rights.contains(rights) {
            return Err(Fault::CannotEscalate);
        }

        let child_id = TokenId::next();
        self.graph.insert_child(parent.token_id, child_id);

        let requested_expiry = ttl.map(|d| Instant::now() + d);
        Ok(CapabilityToken {
            token_id: child_id,
            object_id: parent.object_id,
            rights,
            generation: 0, // fresh node; starts live, independent of parent's own generation
            origin: new_origin,
            expiry: min_expiry(parent.expiry, requested_expiry),
        })
    }

    /// `cap_revoke` — increments the generation counter on `token`'s
    /// revocation-graph node and cascades to every descendant in one graph
    /// walk, `O(k)` in outstanding delegations
    /// (docs/03-kernel-architecture.md §Algorithms).
    pub fn cap_revoke(&mut self, token: &CapabilityToken) -> RevocationReceipt {
        self.graph.revoke(token.token_id)
    }

    /// Which token `token` was derived from, if any. Exposed for audit /
    /// explainability queries ("why does this holder have this authority?"),
    /// not used by the derive/revoke/check algorithms themselves.
    pub fn parent_of(&self, token: &CapabilityToken) -> Option<TokenId> {
        self.graph.parent_of(token.token_id)
    }

    /// True iff `token`'s cached generation still matches its node's live
    /// generation (i.e. neither it nor any ancestor has been revoked since
    /// it was minted/derived) and it has not expired.
    pub fn is_live(&self, token: &CapabilityToken) -> bool {
        self.graph.live_generation(token.token_id) == Some(token.generation) && !token.is_expired()
    }

    fn check_live(&self, token: &CapabilityToken) -> Result<(), Fault> {
        match self.graph.live_generation(token.token_id) {
            None => Err(Fault::NoSuchCapability),
            Some(live) if live != token.generation => Err(Fault::Revoked),
            _ if token.is_expired() => Err(Fault::Expired),
            _ => Ok(()),
        }
    }

    /// Validates a bare token directly (no [`CapabilityTable`] slot lookup),
    /// for callers — like `hyperion-ipc`'s `channel_open` — that receive a
    /// token as a plain argument rather than addressing it via a table.
    pub fn check_rights_ok_result(
        &self,
        token: &CapabilityToken,
        required: RightsMask,
    ) -> Result<(), Fault> {
        self.check_live(token)?;
        if !token.rights.contains(required) {
            return Err(Fault::InsufficientRights);
        }
        Ok(())
    }

    pub fn check_rights_ok(&self, token: &CapabilityToken, required: RightsMask) -> bool {
        self.check_rights_ok_result(token, required).is_ok()
    }

    /// Validates that `table[slot]` holds a live token authorizing `required`,
    /// returning a clone of it. This is `cap_invoke`'s check half
    /// (docs/03-kernel-architecture.md §Pseudocode) without the dispatch
    /// half: this crate does not know about `Object::Device` /
    /// `Object::Thread` / `Object::Endpoint` — those belong to the
    /// subsystems (hyperion-ipc, hyperion-scheduler) that own those object
    /// kinds, which is why they call [`Self::cap_invoke`] with their own
    /// dispatch closure instead of this crate hardcoding a match arm per
    /// kernel-object kind.
    pub fn check(
        &self,
        table: &CapabilityTable,
        slot: SlotIndex,
        required: RightsMask,
    ) -> Result<CapabilityToken, Fault> {
        let token = table.get(slot).ok_or(Fault::NoSuchCapability)?;
        self.check_live(token)?;
        if !token.rights.contains(required) {
            return Err(Fault::InsufficientRights);
        }
        Ok(token.clone())
    }

    /// `cap_invoke` — the only way any code touches a capability-guarded
    /// object. Validates the presented slot, then hands the live token to
    /// `dispatch` (the caller-supplied object handler) exactly once.
    pub fn cap_invoke<T>(
        &self,
        table: &CapabilityTable,
        slot: SlotIndex,
        required: RightsMask,
        dispatch: impl FnOnce(&CapabilityToken) -> T,
    ) -> Result<T, Fault> {
        let token = self.check(table, slot, required)?;
        Ok(dispatch(&token))
    }
}
