//! docs/17 §5's "Provenance Trust Scoring" algorithm, closing this crate's own previously-named
//! "`ProvenanceRecord`/trust-scoring for Knowledge Graph poisoning (T4)" gap: `hyperion-knowledge-
//! graph` already recorded `owner`/`device_origin` per node, and now also records a real
//! `hyperion_knowledge_graph::NodeOrigin` and `corroboration_count` (docs/17 §6's
//! `ProvenanceRecord.origin_type`/`corroboration_count`) -- this module is the real scoring
//! function that combines them (plus age) into the single trust score T4's own mitigation text
//! names: "Context retrieval weights candidate objects by Provenance Trust Score so an
//! untrusted-origin object cannot silently outrank a corroborated one." `hyperion-context::
//! ContextEngine::assemble` is the real consumer -- but as a narrowed, local copy of this exact
//! formula, not a dependency on this crate: `hyperion-security` already transitively depends on
//! `hyperion-context` (`-> hyperion-recovery -> hyperion-agent-runtime -> hyperion-netstack ->
//! hyperion-context`), so the reverse direction is a hard Cargo cycle, confirmed by trying it.
//! Keep `hyperion_context::engine::kg_trust_score` in sync by hand with this module's own
//! constants if either ever changes.
//!
//! **Origin base score.** `UserAuthored` is the most trusted (a human directly created this);
//! `AgentGenerated` next (an Agent acting under a real, already-capability-checked dispatch, not
//! arbitrary external content); `SyncedRemote` next (another of *this same owner's* devices, per
//! `NodeOrigin`'s own doc comment on why this is a separate axis from `owner` -- still real
//! provenance, just not locally authored); `IngestedExternal` lowest -- docs/17 T4's own named
//! attack: "plant a malicious Semantic Object" via "a shared document, an email attachment, a
//! synced folder." This is also `NodeOrigin`'s own conservative `Default`, so a node with no real
//! provenance recorded scores as if it were the least-trusted case, never a guessed-higher one.
//!
//! **Corroboration.** Each independent reconfirmation
//! (`hyperion_knowledge_graph::KnowledgeGraph::corroborate_node`) raises the score, saturating
//! rather than growing unbounded -- docs/17 T4's own "cannot silently outrank a corroborated one"
//! names exactly this: a repeatedly-confirmed object should be able to overcome a merely
//! lower-tier origin, but a single corroboration shouldn't instantly launder an ingested object
//! into full trust either.
//!
//! **Age.** Deliberately modeled as *increasing* trust up to a grace-period ceiling, not decaying
//! it: docs/17 §5 names "age-based decay" as an input without specifying its direction, and unlike
//! `hyperion-knowledge-graph::decay`'s edge-confidence decay (where an unconfirmed *inferred*
//! relationship should fade), a malicious injection is most likely to be flagged and remediated
//! soon after it lands -- an object that has survived unflagged for a real, meaningful window has
//! passed more real scrutiny than one created moments ago, not less. A freshly created node is not
//! penalized to zero, only denied this maturity bonus until it has actually aged past the window.
//!
//! **Composition and floor.** The final score is bounded to `[0.5, 1.0]`, never lower: T4's own
//! mitigation is phrased as demotion ("cannot silently outrank"), not exclusion -- a caller
//! layering this as a multiplicative re-rank weight (as `hyperion-context` does) must never make
//! a legitimate, if untrusted-origin, candidate wholly invisible, only outranked by better-attested
//! ones.

use hyperion_knowledge_graph::{NodeOrigin, NodeRecord};

/// How many independent corroborations fully saturate the corroboration bonus -- a real,
/// deliberately small number: docs/17 T4's own framing is about a single malicious plant, so a
/// handful of independent reconfirmations already meaningfully distinguishes it from one.
const CORROBORATION_SATURATION: f32 = 5.0;
/// The maximum score contribution corroboration alone can add, atop the origin base score.
const CORROBORATION_MAX_BONUS: f32 = 0.3;
/// How long a node must exist, unflagged, before it earns the full age-based maturity bonus --
/// a real, deliberately short window for this hosted simulator's own timescale (docs/41's own
/// "dozens of objects, not thousands" framing extends naturally to "hours, not months" here too).
const AGE_MATURITY_SECS: f32 = 24.0 * 3600.0;
const AGE_MAX_BONUS: f32 = 0.15;
/// This module's own doc comment: T4 demotes, it never excludes.
const MIN_TRUST_SCORE: f32 = 0.5;

fn origin_base_score(origin: NodeOrigin) -> f32 {
    match origin {
        NodeOrigin::UserAuthored => 1.0,
        NodeOrigin::AgentGenerated => 0.85,
        NodeOrigin::SyncedRemote => 0.65,
        NodeOrigin::IngestedExternal => 0.4,
    }
}

fn corroboration_bonus(corroboration_count: u32) -> f32 {
    (corroboration_count as f32 / CORROBORATION_SATURATION).min(1.0) * CORROBORATION_MAX_BONUS
}

fn age_bonus(created_at: u64, now: u64) -> f32 {
    let age_secs = now.saturating_sub(created_at) as f32;
    (age_secs / AGE_MATURITY_SECS).min(1.0) * AGE_MAX_BONUS
}

/// docs/17 §5's real Provenance Trust Score for one Knowledge Graph node, `0.0..=1.0` in
/// principle but never returned below [`MIN_TRUST_SCORE`] -- see this module's own doc comment
/// for the full real reasoning behind each term and that floor.
pub fn kg_trust_score(node: &NodeRecord, now: u64) -> f32 {
    let raw = origin_base_score(node.origin)
        + corroboration_bonus(node.corroboration_count)
        + age_bonus(node.created_at, now);
    raw.clamp(MIN_TRUST_SCORE, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(origin: NodeOrigin, corroboration_count: u32, created_at: u64) -> NodeRecord {
        NodeRecord {
            object_type: "note".to_string(),
            embedding: None,
            metadata: serde_json::json!({}),
            owner: 1,
            device_origin: 0,
            origin,
            corroboration_count,
            tenant_id: Default::default(),
            created_at,
            updated_at: created_at,
            tombstone: false,
        }
    }

    #[test]
    fn user_authored_outranks_ingested_external_all_else_equal() {
        let user = node(NodeOrigin::UserAuthored, 0, 1_000);
        let ingested = node(NodeOrigin::IngestedExternal, 0, 1_000);
        assert!(kg_trust_score(&user, 1_000) > kg_trust_score(&ingested, 1_000));
    }

    #[test]
    fn a_freshly_ingested_object_never_scores_above_a_corroborated_one() {
        let fresh_ingested = node(NodeOrigin::IngestedExternal, 0, 1_000);
        let corroborated_ingested = node(NodeOrigin::IngestedExternal, 5, 1_000);
        assert!(
            kg_trust_score(&corroborated_ingested, 1_000) > kg_trust_score(&fresh_ingested, 1_000),
            "a repeatedly-confirmed object must score higher than an unconfirmed one of the same \
             origin -- T4's own 'cannot silently outrank a corroborated one'"
        );
    }

    #[test]
    fn corroboration_saturates_rather_than_growing_unbounded() {
        let at_cap = node(NodeOrigin::IngestedExternal, 5, 1_000);
        let past_cap = node(NodeOrigin::IngestedExternal, 500, 1_000);
        assert_eq!(
            kg_trust_score(&at_cap, 1_000),
            kg_trust_score(&past_cap, 1_000)
        );
    }

    #[test]
    fn an_object_that_has_aged_past_the_maturity_window_scores_higher_than_a_fresh_one() {
        let fresh = node(NodeOrigin::IngestedExternal, 0, 1_000);
        let matured = node(NodeOrigin::IngestedExternal, 0, 1_000);
        let now = 1_000 + AGE_MATURITY_SECS as u64 + 1;
        assert!(kg_trust_score(&matured, now) > kg_trust_score(&fresh, 1_000));
    }

    #[test]
    fn the_score_never_falls_below_the_real_demotion_floor() {
        let worst_case = node(NodeOrigin::IngestedExternal, 0, 1_000);
        assert!(kg_trust_score(&worst_case, 1_000) >= MIN_TRUST_SCORE);
    }

    #[test]
    fn the_score_never_exceeds_one() {
        let best_case = node(NodeOrigin::UserAuthored, 1000, 1_000);
        let now = 1_000 + (AGE_MATURITY_SECS as u64) * 100;
        assert!(kg_trust_score(&best_case, now) <= 1.0);
    }
}
