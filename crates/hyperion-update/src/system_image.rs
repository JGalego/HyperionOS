use std::sync::Mutex;

use crate::types::{SystemImageSlot, SystemImageSlotName, UpdateError, Version};

const DEFAULT_BOOT_ATTEMPTS: u8 = 3;

fn other(slot: SystemImageSlotName) -> SystemImageSlotName {
    match slot {
        SystemImageSlotName::A => SystemImageSlotName::B,
        SystemImageSlotName::B => SystemImageSlotName::A,
    }
}

/// docs/32 §5's system-image A/B slot machine — "the one exception:
/// system image rollback never calls `restore_to` at all," because the
/// Storage Engine's four stores aren't slot-scoped; they're the same
/// live data regardless of booted image. Rollback here is a pure
/// pointer flip, deliberately kept out of `hyperion-recovery` entirely —
/// see this crate's doc comment.
pub struct SystemImageController {
    slots: Mutex<[SystemImageSlot; 2]>,
    active: Mutex<SystemImageSlotName>,
}

impl SystemImageController {
    pub fn new(initial_version: Version) -> Self {
        SystemImageController {
            slots: Mutex::new([
                SystemImageSlot {
                    slot: SystemImageSlotName::A,
                    version: initial_version,
                    boot_attempts_remaining: DEFAULT_BOOT_ATTEMPTS,
                    committed: true,
                },
                SystemImageSlot {
                    slot: SystemImageSlotName::B,
                    version: 0,
                    boot_attempts_remaining: 0,
                    committed: false,
                },
            ]),
            active: Mutex::new(SystemImageSlotName::A),
        }
    }

    fn slot_mut(
        slots: &mut [SystemImageSlot; 2],
        name: SystemImageSlotName,
    ) -> &mut SystemImageSlot {
        slots
            .iter_mut()
            .find(|s| s.slot == name)
            .expect("both slots always exist")
    }

    pub fn active_slot(&self) -> SystemImageSlot {
        let active = *self.active.lock().unwrap();
        *self
            .slots
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.slot == active)
            .expect("both slots always exist")
    }

    /// Stages a new version into whichever slot is *not* currently
    /// active — the active slot, and the live data it boots into, is
    /// never touched until [`Self::commit`].
    pub fn stage_to_inactive_slot(&self, version: Version) -> SystemImageSlotName {
        let inactive = other(*self.active.lock().unwrap());
        let mut slots = self.slots.lock().unwrap();
        let slot = Self::slot_mut(&mut slots, inactive);
        slot.version = version;
        slot.boot_attempts_remaining = DEFAULT_BOOT_ATTEMPTS;
        slot.committed = false;
        inactive
    }

    /// docs/32 §6's boot-attempt-counter exhaustion: each attempt
    /// consumes one of the staged slot's remaining attempts; running out
    /// before [`Self::commit`] is called is the signal a caller uses to
    /// give up and keep booting the still-active slot — this function
    /// never flips `active` itself, only [`Self::commit`] does.
    pub fn attempt_boot(&self, slot_name: SystemImageSlotName) -> Result<(), UpdateError> {
        let mut slots = self.slots.lock().unwrap();
        let slot = Self::slot_mut(&mut slots, slot_name);
        if slot.boot_attempts_remaining == 0 {
            return Err(UpdateError::BootAttemptsExhausted);
        }
        slot.boot_attempts_remaining -= 1;
        Ok(())
    }

    /// A successful boot commits the staged slot as active — the point
    /// past which this slot is trusted and future rollback would need a
    /// fresh staging cycle, not a simple revert.
    pub fn commit(&self, slot_name: SystemImageSlotName) {
        let mut slots = self.slots.lock().unwrap();
        Self::slot_mut(&mut slots, slot_name).committed = true;
        drop(slots);
        *self.active.lock().unwrap() = slot_name;
    }
}
