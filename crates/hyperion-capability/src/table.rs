use crate::token::CapabilityToken;
use crate::types::TrustBoundaryId;

/// A slot handle into a [`CapabilityTable`] — the small integer a Trust
/// Boundary actually addresses, mirroring seL4-style CPtrs. Holding a
/// `SlotIndex` alone grants nothing; it is only meaningful paired with the
/// specific table it was issued from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotIndex(pub usize);

/// Per-Trust-Boundary capability table; the *only* way a process addresses
/// anything, including its own memory, per
/// docs/03-kernel-architecture.md §Data Structures.
#[derive(Debug)]
pub struct CapabilityTable {
    slots: Vec<Option<CapabilityToken>>,
    boundary: TrustBoundaryId,
    parent: Option<TrustBoundaryId>,
}

impl CapabilityTable {
    pub fn new(boundary: TrustBoundaryId, parent: Option<TrustBoundaryId>) -> Self {
        CapabilityTable {
            slots: Vec::new(),
            boundary,
            parent,
        }
    }

    pub fn boundary(&self) -> TrustBoundaryId {
        self.boundary
    }

    pub fn parent(&self) -> Option<TrustBoundaryId> {
        self.parent
    }

    /// Installs a token into the first free slot, growing the table if none
    /// is free. Returns the handle the boundary will use to address it.
    pub fn insert(&mut self, token: CapabilityToken) -> SlotIndex {
        if let Some(i) = self.slots.iter().position(Option::is_none) {
            self.slots[i] = Some(token);
            return SlotIndex(i);
        }
        self.slots.push(Some(token));
        SlotIndex(self.slots.len() - 1)
    }

    pub fn get(&self, slot: SlotIndex) -> Option<&CapabilityToken> {
        self.slots.get(slot.0).and_then(|s| s.as_ref())
    }

    /// Clears a slot, e.g. after the holder itself gives up the capability.
    /// This does not revoke it for other holders — see
    /// [`crate::CapabilityMonitor::cap_revoke`] for that.
    pub fn remove(&mut self, slot: SlotIndex) -> Option<CapabilityToken> {
        self.slots.get_mut(slot.0).and_then(Option::take)
    }

    pub fn len(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
