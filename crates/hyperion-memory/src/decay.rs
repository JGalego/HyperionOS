use crate::types::{MemoryRecord, MemoryTier};

/// docs/08 §5.3: `score >= THETA_PROMOTE` (and not already Long-Term)
/// promotes a record to the durable archive tier.
pub const THETA_PROMOTE: f32 = 0.8;
/// docs/08 §5.3: `score < THETA_ARCHIVE` moves a record to the "Dormant"
/// funnel stage — deprioritized in default recall, never deleted.
pub const THETA_ARCHIVE: f32 = 0.2;

const W_RECENCY: f64 = 0.5;
const W_FREQUENCY: f64 = 0.3;
const W_IMPORTANCE: f64 = 0.2;
/// A fixed normalization constant for `F(r) = log(1+access_count) /
/// log(1+access_count_norm)` — docs/08 §5.2 doesn't pin this down
/// concretely; 20 accesses saturating `F(r)` to 1.0 is a reasonable stand-in
/// pending real field telemetry to tune it against.
const ACCESS_COUNT_NORM: f64 = 20.0;

/// docs/08 §5.2's `score(r, t)` — a weighted decay function, not a TTL.
/// `now` and `record.last_accessed_at` are both Unix seconds.
pub fn decay_score(record: &MemoryRecord, now: u64) -> f32 {
    if record.pinned {
        return 1.0; // unconditional; recency term never applies
    }

    let delta_t = now.saturating_sub(record.last_accessed_at) as f64;
    let tau = record.tier.tau_seconds();
    let recency = if tau.is_finite() {
        (-delta_t / tau).exp()
    } else {
        1.0
    };

    let frequency = (1.0 + record.access_count as f64).ln() / (1.0 + ACCESS_COUNT_NORM).ln();
    let frequency = frequency.min(1.0);

    let importance = record.importance.clamp(0.0, 1.0) as f64;

    (W_RECENCY * recency + W_FREQUENCY * frequency + W_IMPORTANCE * importance) as f32
}

/// docs/08 §5.3: only non-Long-Term records are candidates for promotion —
/// Long-Term is the terminus, not a station on the way to itself.
pub(crate) fn is_promotable(tier: MemoryTier) -> bool {
    !matches!(tier, MemoryTier::LongTerm)
}
