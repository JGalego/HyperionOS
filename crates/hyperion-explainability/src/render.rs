use hyperion_capability::{CapabilityMonitor, CapabilityToken};

use crate::store::{resolve_by_action, ExplanationStore};
use crate::types::{ActionId, Depth, ExplainabilityError, ExplanationRecord, ExplanationView};

fn headline_for(record: &ExplanationRecord) -> String {
    let confidence = record
        .confidence
        .map(|c| format!("{:.0}%", c.value * 100.0))
        .unwrap_or_else(|| "unknown".to_string());
    format!(
        "Agent {} used '{}' for intent {} ({} step(s), confidence {confidence})",
        record.agent_id,
        record.capability_ref,
        record.triggering_intent_id,
        record.reasoning_chain.len(),
    )
}

/// docs/18 §5/§6's `resolve_why`/`explain.query`: never requires the
/// caller to already know an internal record id, only the `action_id`
/// the effect was attached to. `Depth::Headline` renders a single
/// sentence (docs/18 §12: "a beginner sees the headline and undo
/// button"); `Depth::Full` additionally resolves the full record and
/// walks `parent_records` depth-first, matching docs/18 §5's multi-agent
/// merge resolution order (root first, expand on request). Real natural-
/// language generation is deferred — see this crate's doc comment; this
/// is a deterministic template, not a model call. Capability-checked and
/// Trust-Boundary-filtered (2026-07-16): this crate's own literal `explain.query`
/// implementation previously took no `monitor`/`token` at all, directly contradicting docs/18
/// §8's "access to an `explain.query` result is gated by the same capability grant that gated
/// the underlying data" and §13's explicit call for a test proving it. `resolve_by_action`/
/// `Self::get`'s own real gating (`ExplanationStore::get`) is reused here rather than a second
/// check invented in this module — a record (or a parent record, when walking `Depth::Full`)
/// outside the caller's own Trust Boundary is silently excluded, never an error.
pub fn resolve_why(
    store: &ExplanationStore,
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    action_id: ActionId,
    depth: Depth,
) -> Result<Option<ExplanationView>, ExplainabilityError> {
    let Some(record) = resolve_by_action(store, monitor, token, action_id)? else {
        return Ok(None);
    };
    let headline = headline_for(&record);

    let mut parents = Vec::new();
    if depth == Depth::Full {
        for parent_id in &record.parent_records {
            if let Some(parent_record) = store.get(monitor, token, *parent_id)? {
                if let Some(view) =
                    resolve_why(store, monitor, token, parent_record.action_id, Depth::Full)?
                {
                    parents.push(view);
                }
            }
        }
    }

    Ok(Some(ExplanationView {
        headline,
        full: if depth == Depth::Full {
            Some(record)
        } else {
            None
        },
        parents,
    }))
}
