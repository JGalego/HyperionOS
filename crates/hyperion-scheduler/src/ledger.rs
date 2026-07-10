use crate::types::ResourceDimension;

/// Per-resource-dimension ledger; one instance per physical/thermal domain,
/// per docs/04-scheduler.md §Data Structures.
///
/// The full spec also has a thermal/battery feedback governor that scales
/// `capacity` down from its nameplate maximum each epoch (§Algorithms 3).
/// That requires real sensor input this hosted simulator has no hardware to
/// provide, so `capacity` is fixed here — the governor is a Phase-2+
/// (bare-metal bring-up) concern layered on top of this same struct, not a
/// change to its shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLedger {
    pub dimension: ResourceDimension,
    pub capacity: u32,
    pub reserved_for_realtime: u32,
    pub allocated: u32,
    pub epoch: u64,
}

impl ResourceLedger {
    pub fn new(dimension: ResourceDimension, capacity: u32, reserved_for_realtime: u32) -> Self {
        ResourceLedger {
            dimension,
            capacity,
            reserved_for_realtime: reserved_for_realtime.min(capacity),
            allocated: 0,
            epoch: 0,
        }
    }

    /// Capacity a task may draw against: the full nameplate for
    /// `RealTimeUI` (`use_reserved = true`), or capacity minus the
    /// real-time reservation for every other class — docs/04 §Algorithms 1.
    pub fn headroom(&self, use_reserved: bool) -> u32 {
        if use_reserved {
            self.capacity
        } else {
            self.capacity.saturating_sub(self.reserved_for_realtime)
        }
    }
}
