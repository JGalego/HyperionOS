use std::cmp::Ordering;
use std::collections::HashSet;

use hyperion_capability::{CapabilityMonitor, CapabilityToken};
use hyperion_knowledge_graph::{GraphError, GraphQuery, KnowledgeGraph, NodeId};

use crate::types::ExtractedEntity;

/// docs/19 §5.4's merge threshold. This crate has no embedding producer to
/// score similarity with — Phase 3's Local AI Runtime embeddings were
/// never wired into a web-extraction pipeline — so a normalized
/// title/name token-overlap ratio stands in for "high-confidence
/// embedding similarity." See this crate's doc comment; this is a
/// materially cruder proxy than the rest of this workspace's "pass a
/// pre-computed `Vec<f32>`" deferral pattern, called out explicitly rather
/// than dressed up as equivalent.
pub(crate) const MERGE_THRESHOLD: f32 = 0.6;
pub(crate) const DISTINCT_FLOOR: f32 = 0.2;

pub(crate) enum MatchDecision {
    /// Exact external identifier match, or token overlap at/above
    /// [`MERGE_THRESHOLD`] — safe to merge into the existing node.
    ConfidentMatch(NodeId),
    /// Token overlap between [`DISTINCT_FLOOR`] and [`MERGE_THRESHOLD`] —
    /// docs/19 §5.4: "not silently merged." A new node is created and
    /// flagged `needs_review`; the near-match id is carried for audit.
    Ambiguous(NodeId, f32),
    Distinct,
}

fn identifier_of(metadata: &serde_json::Value) -> Option<&str> {
    metadata.get("identifier").and_then(|v| v.as_str())
}

fn title_of(metadata: &serde_json::Value) -> String {
    metadata
        .get("title")
        .or_else(|| metadata.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase()
}

fn token_overlap(a: &str, b: &str) -> f32 {
    let ta: HashSet<&str> = a.split_whitespace().collect();
    let tb: HashSet<&str> = b.split_whitespace().collect();
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let intersection = ta.intersection(&tb).count() as f32;
    let union = ta.union(&tb).count() as f32;
    intersection / union
}

/// docs/19 §5.4, in priority order: (a) exact external identifier, (b) a
/// token-overlap similarity proxy (see this module's doc comment), (c) no
/// match. Only same-type nodes (the query's own `type_filter`) are ever
/// compared, matching docs/19's implicit assumption that a Paper is never
/// matched against a Person.
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
                token_overlap(&candidate_title, &title_of(&h.node.metadata)),
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
