use std::cmp::Ordering;

use hyperion_ai_runtime::{cosine_similarity, embed};
use hyperion_capability::{CapabilityMonitor, CapabilityToken};
use hyperion_knowledge_graph::{GraphError, GraphQuery, KnowledgeGraph, NodeId};

use crate::types::ExtractedEntity;

/// docs/19 §5.4's merge threshold, now scored against a real embedding similarity
/// (`hyperion_ai_runtime::embed`/`cosine_similarity`) rather than a token-overlap-ratio proxy —
/// closes this crate's own previously-named "no embedding producer exists in this pipeline" gap.
/// Recalibrated from the token-overlap era's `0.6`: a bag-of-words cosine similarity over a
/// short title scores purely on shared-word fraction with no semantic weighting, so two
/// short phrases sharing most of their words but differing in one meaningful term (e.g. "John
/// Smith Engineer" vs. "John Smith Artist" — two different real people) already land around
/// `0.67`, well above the old Jaccard-tuned threshold — confirmed empirically (this crate's own
/// `a_near_duplicate_title_with_no_identifier_is_provisional_not_silently_merged` test), not
/// assumed. `0.9` requires near-total lexical overlap before treating two titles as confidently
/// the same real-world entity.
pub(crate) const MERGE_THRESHOLD: f32 = 0.9;
pub(crate) const DISTINCT_FLOOR: f32 = 0.2;

pub(crate) enum MatchDecision {
    /// Exact external identifier match, or embedding similarity at/above
    /// [`MERGE_THRESHOLD`] — safe to merge into the existing node.
    ConfidentMatch(NodeId),
    /// Embedding similarity between [`DISTINCT_FLOOR`] and [`MERGE_THRESHOLD`] —
    /// docs/19 §5.4: "not silently merged." A new node is created and
    /// flagged `needs_review`; the near-match id is carried for audit.
    Ambiguous(NodeId, f32),
    Distinct,
}

fn identifier_of(metadata: &serde_json::Value) -> Option<&str> {
    metadata.get("identifier").and_then(|v| v.as_str())
}

pub(crate) fn title_of(metadata: &serde_json::Value) -> String {
    metadata
        .get("title")
        .or_else(|| metadata.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase()
}

fn embedding_similarity(a: &str, b: &str) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    cosine_similarity(&embed(a), &embed(b))
}

/// docs/19 §5.4, in priority order: (a) exact external identifier, (b) a real embedding
/// similarity (see this module's doc comment), (c) no match. Only same-type nodes (the query's
/// own `type_filter`) are ever compared, matching docs/19's implicit assumption that a Paper is
/// never matched against a Person.
pub(crate) fn find_match(
    graph: &KnowledgeGraph,
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    candidate: &ExtractedEntity,
) -> Result<MatchDecision, GraphError> {
    let query = GraphQuery {
        type_filter: Some(vec![candidate.entity_type.as_object_type().to_string()]),
        ..Default::default()
    };
    let hits = graph.query(monitor, token, &query)?;

    if let Some(candidate_id) = &candidate.identifier {
        if let Some(hit) = hits
            .iter()
            .find(|h| identifier_of(&h.node.metadata) == Some(candidate_id.as_str()))
        {
            return Ok(MatchDecision::ConfidentMatch(hit.node_id));
        }
    }

    let candidate_title = title_of(&candidate.fields);
    if candidate_title.is_empty() {
        return Ok(MatchDecision::Distinct);
    }

    let best = hits
        .iter()
        .map(|h| {
            (
                h.node_id,
                embedding_similarity(&candidate_title, &title_of(&h.node.metadata)),
            )
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));

    Ok(match best {
        Some((node_id, score)) if score >= MERGE_THRESHOLD => {
            MatchDecision::ConfidentMatch(node_id)
        }
        Some((node_id, score)) if score > DISTINCT_FLOOR => {
            MatchDecision::Ambiguous(node_id, score)
        }
        _ => MatchDecision::Distinct,
    })
}
