//! docs/08-memory-engine.md §5.2/§13: "a pinned record's decay_score never
//! drops below 1.0 regardless of simulated time or access patterns," and
//! the promote/dormant funnel thresholds.

use hyperion_memory::{decay_score, MemoryRecord, MemoryTier, THETA_ARCHIVE, THETA_PROMOTE};
use proptest::prelude::*;

fn base_record(
    tier: MemoryTier,
    pinned: bool,
    importance: f32,
    access_count: u32,
    last_accessed_at: u64,
) -> MemoryRecord {
    MemoryRecord {
        id: hyperion_storage::ObjectId(0),
        tier,
        content: serde_json::json!({}),
        embedding: None,
        created_at: 0,
        last_accessed_at,
        access_count,
        importance,
        decay_score: 0.0,
        pinned,
        provenance: Vec::new(),
        erased: false,
        dormant: false,
    }
}

proptest! {
    #[test]
    fn pinned_record_decay_score_never_drops_below_one(
        importance in 0.0f32..=1.0,
        access_count in 0u32..1000,
        last_accessed_at in 0u64..1_000_000,
        now in 0u64..10_000_000,
        tier_idx in 0u8..4,
    ) {
        let tier = match tier_idx {
            0 => MemoryTier::Episodic,
            1 => MemoryTier::Semantic,
            2 => MemoryTier::Procedural,
            _ => MemoryTier::LongTerm,
        };
        let record = base_record(tier, true, importance, access_count, last_accessed_at);
        prop_assert_eq!(decay_score(&record, now), 1.0);
    }
}

#[test]
fn fresh_high_importance_record_scores_above_the_promote_threshold() {
    let record = base_record(MemoryTier::Semantic, false, 1.0, 25, 100);
    let score = decay_score(&record, 100);
    assert!(score >= THETA_PROMOTE, "score was {score}");
}

#[test]
fn stale_low_importance_record_scores_below_the_archive_threshold() {
    let one_year_secs = 365 * 24 * 3600;
    let record = base_record(MemoryTier::Semantic, false, 0.0, 0, 0);
    let score = decay_score(&record, one_year_secs);
    assert!(score < THETA_ARCHIVE, "score was {score}");
}

#[test]
fn long_term_records_do_not_decay_with_age() {
    let record = base_record(MemoryTier::LongTerm, false, 0.5, 5, 0);
    let fresh_score = decay_score(&record, 0);
    let old_score = decay_score(&record, 365 * 24 * 3600);
    assert_eq!(
        fresh_score, old_score,
        "Long-Term tau is infinite — age must not move the score"
    );
}
