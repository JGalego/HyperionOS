use crate::types::{EdgeOrigin, EdgeRecord};

/// docs/09 §5.2's own inferred-edge decay: "an inferred edge, unlike an explicit one, is a
/// hypothesis and is allowed to fade" using "the same recency-weighted mechanism as
/// [08 — Memory Engine §5.2]" — matches `hyperion-memory::decay::decay_score`'s own tau for its
/// Semantic/Procedural tiers (30 days), rather than inventing an unrelated constant, since a
/// `co-occurs-with`/`semantically-similar-to` edge is that same crate's own inferred-fact
/// concept, just materialized here as a graph edge instead of a memory record.
pub const DEFAULT_INFERRED_EDGE_TAU_SECS: f64 = 30.0 * 24.0 * 3600.0;

/// docs/09 §5.2's previously-named decay gap, closed for real: the real, on-demand decayed
/// weight of `edge` at real time `now`, using [`EdgeRecord::last_confirmed_at`] (never
/// `created_at`, which stays fixed at an edge's original creation and would understate how
/// recently it was last reconfirmed). An [`EdgeOrigin::Explicit`] edge never decays -- it isn't a
/// hypothesis, per docs/09 §5.2's own framing -- so this returns its stored `weight` unchanged
/// regardless of `now`. Mirrors `hyperion-memory::decay::decay_score`'s own pure,
/// recompute-from-scratch-every-call shape (never a batch job that overwrites `weight` in
/// place): repeated calls with an unchanged `edge` and advancing `now` return a smoothly
/// shrinking value, and a fresh reconfirmation (a real `KnowledgeGraph::link` call, which
/// advances `last_confirmed_at`) genuinely restores full strength on the very next call.
pub fn effective_edge_weight(edge: &EdgeRecord, now: u64) -> f32 {
    if edge.origin == EdgeOrigin::Explicit {
        return edge.weight;
    }
    let delta_t = now.saturating_sub(edge.last_confirmed_at) as f64;
    let recency = (-delta_t / DEFAULT_INFERRED_EDGE_TAU_SECS).exp();
    edge.weight * recency as f32
}
