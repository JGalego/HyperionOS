use hyperion_capability::{CapabilityMonitor, CapabilityToken};

use crate::store::ExplanationStore;
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
/// the underlying data" and §13's explicit call for a test proving it. `Self::get`'s own real
/// gating (`ExplanationStore::get`) is reused here rather than a second check invented in this
/// module — a record (or a parent record, when walking `Depth::Full`) outside the caller's own
/// Trust Boundary is silently excluded, never an error. docs/18 §9's degrade path is real too
/// (2026-07-17): `ExplanationStore::get_or_reconstruct_by_action` is tried instead of a plain
/// store lookup, and [`ExplanationView::reconstructed`] surfaces whether the result actually came
/// from [31 — Event System](../31-event-system.md) replay rather than the authoritative,
/// causally-recorded store — never silently presented as the same thing.
pub fn resolve_why(
    store: &ExplanationStore,
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    action_id: ActionId,
    depth: Depth,
) -> Result<Option<ExplanationView>, ExplainabilityError> {
    let Some(lookup) = store.get_or_reconstruct_by_action(monitor, token, action_id)? else {
        return Ok(None);
    };
    let reconstructed = lookup.is_reconstructed();
    let record = lookup.record().clone();
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
        reconstructed,
    }))
}
