use std::collections::HashMap;

use crate::types::{ResidencyEntry, ResidencyStatus};

/// docs/22 §5.2's page-replacement-style Residency Manager: evict the least
/// valuable `Cold`-eligible candidate (`value = recency * predicted_next_use`)
/// when a newly needed model doesn't fit; `pin_count` overrides eviction
/// entirely.
#[derive(Debug, Default)]
pub(crate) struct ResidencyManager {
    entries: HashMap<u64, ResidencyEntry>,
    /// Footprint of whatever's currently resident (`Hot`/`Warm`), keyed by
    /// model id — the only place this manager needs to know footprint at
    /// all is to keep `used_mb` correct across load/evict cycles.
    footprints: HashMap<u64, u32>,
    used_mb: u32,
}

impl ResidencyManager {
    pub(crate) fn capacity_available(&self, total_mb: u32) -> u32 {
        total_mb.saturating_sub(self.used_mb)
    }

    pub(crate) fn is_hot(&self, model_id: u64) -> bool {
        self.entries
            .get(&model_id)
            .is_some_and(|e| e.status == ResidencyStatus::Hot)
    }

    pub(crate) fn touch(&mut self, model_id: u64, now: u64) {
        if let Some(entry) = self.entries.get_mut(&model_id) {
            entry.last_used = now;
        }
    }

    pub(crate) fn pin(&mut self, model_id: u64) {
        if let Some(entry) = self.entries.get_mut(&model_id) {
            entry.pin_count += 1;
        }
    }

    pub(crate) fn unpin(&mut self, model_id: u64) {
        if let Some(entry) = self.entries.get_mut(&model_id) {
            entry.pin_count = entry.pin_count.saturating_sub(1);
        }
    }

    pub(crate) fn set_predicted_next_use(&mut self, model_id: u64, value: f32) {
        if let Some(entry) = self.entries.get_mut(&model_id) {
            entry.predicted_next_use = value;
        }
    }

    pub(crate) fn entry(&self, model_id: u64) -> Option<ResidencyEntry> {
        self.entries.get(&model_id).copied()
    }

    /// Ensures `footprint_mb` of capacity is free (within `total_mb`) by
    /// evicting the lowest-value non-pinned resident entries — docs/22
    /// §5.2. Returns `false` if even evicting everything evictable can't
    /// make room (§Failure Modes' "out of memory").
    pub(crate) fn make_room(&mut self, footprint_mb: u32, total_mb: u32, now: u64) -> bool {
        while self.capacity_available(total_mb) < footprint_mb {
            let victim = self
                .entries
                .values()
                .filter(|e| e.status != ResidencyStatus::Cold && e.pin_count == 0)
                .min_by(|a, b| {
                    let value = |e: &ResidencyEntry| {
                        let age = (now.saturating_sub(e.last_used)) as f32 + 1.0;
                        (1.0 / age) * (e.predicted_next_use + 0.001)
                    };
                    value(a)
                        .partial_cmp(&value(b))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|e| e.model_id);
            match victim {
                Some(id) => self.evict(id),
                None => return false,
            }
        }
        true
    }

    pub(crate) fn mark_hot(&mut self, model_id: u64, footprint_mb: u32, now: u64) {
        let was_resident = self
            .entries
            .get(&model_id)
            .is_some_and(|e| e.status != ResidencyStatus::Cold);
        if !was_resident {
            self.used_mb += footprint_mb;
            self.footprints.insert(model_id, footprint_mb);
        }
        self.entries
            .entry(model_id)
            .and_modify(|e| {
                e.status = ResidencyStatus::Hot;
                e.last_used = now;
            })
            .or_insert(ResidencyEntry {
                model_id,
                status: ResidencyStatus::Hot,
                last_used: now,
                pin_count: 0,
                predicted_next_use: 0.0,
            });
    }

    fn evict(&mut self, model_id: u64) {
        if let Some(entry) = self.entries.get_mut(&model_id) {
            entry.status = ResidencyStatus::Cold;
        }
        if let Some(freed) = self.footprints.remove(&model_id) {
            self.used_mb = self.used_mb.saturating_sub(freed);
        }
    }
}
