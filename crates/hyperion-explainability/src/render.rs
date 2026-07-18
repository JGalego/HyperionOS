use hyperion_capability::{CapabilityMonitor, CapabilityToken};
use hyperion_knowledge_graph::{KnowledgeGraph, NodeId};

use crate::store::ExplanationStore;
use crate::types::{
    ActionId, Depth, ExplainabilityError, ExplanationRecord, ExplanationView, ResolvedEvidence,
    ResolvedReasoningStep,
};

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
        resolved_reasoning_chain: Vec::new(),
        resolved_evidence: Vec::new(),
    }))
}

/// A single reference's real, human-readable description via
/// `hyperion_knowledge_graph::NodeRecord::display_label` -- an honest placeholder, never an
/// error, when the referenced node is no longer visible to the caller's own capability token
/// (tombstoned, or genuinely never existed under a stale/malformed id). A historical reasoning
/// step naming an object that has since been deleted is expected drift, not a bug worth failing
/// the whole explanation over.
fn resolve_ref(
    graph: &KnowledgeGraph,
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    node_id: NodeId,
) -> String {
    match graph.get(monitor, token, node_id) {
        Ok(node) => node.display_label(),
        Err(_) => "a reference that's no longer available".to_string(),
    }
}

fn resolve_refs_in_view(
    view: &mut ExplanationView,
    graph: &KnowledgeGraph,
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
) {
    if let Some(record) = &view.full {
        view.resolved_reasoning_chain = record
            .reasoning_chain
            .iter()
            .map(|step| ResolvedReasoningStep {
                step_index: step.step_index,
                description: step.description.clone(),
                inputs: step
                    .inputs_ref
                    .iter()
                    .map(|id| resolve_ref(graph, monitor, token, *id))
                    .collect(),
                output: step
                    .output_ref
                    .map(|id| resolve_ref(graph, monitor, token, id)),
            })
            .collect();
        view.resolved_evidence = record
            .evidence
            .iter()
            .map(|e| ResolvedEvidence {
                label: resolve_ref(graph, monitor, token, e.object_id),
                excerpt_or_summary: e.excerpt_or_summary.clone(),
                weight: e.weight,
            })
            .collect();
    }
    for parent in &mut view.parents {
        resolve_refs_in_view(parent, graph, monitor, token);
    }
}

/// This crate's own previously-named "`ReasoningStep.inputs_ref`/`output_ref` and
/// `EvidenceRef.object_id` never resolved via the Knowledge Graph" gap, closed for real:
/// [`resolve_why`]'s own output (unchanged, still callable standalone) plus every reference it
/// carries -- at every depth of the `Depth::Full` parent chain, not just the root -- resolved
/// into a real human-readable [`crate::types::ResolvedReasoningStep`]/
/// [`crate::types::ResolvedEvidence`] via `hyperion_knowledge_graph::NodeRecord::display_label`.
/// A caller with a real `KnowledgeGraph` handle (every crate in this workspace that constructs a
/// `ReasoningStep` already holds one -- see this crate's own doc comment) should call this
/// instead of the bare [`resolve_why`] so a person reading an explanation never sees a raw
/// `NodeId` integer, the same "never expose... internals directly" CLAUDE.md already asks of
/// every other surface in this workspace.
pub fn resolve_why_with_graph(
    store: &ExplanationStore,
    graph: &KnowledgeGraph,
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    action_id: ActionId,
    depth: Depth,
) -> Result<Option<ExplanationView>, ExplainabilityError> {
    let Some(mut view) = resolve_why(store, monitor, token, action_id, depth)? else {
        return Ok(None);
    };
    resolve_refs_in_view(&mut view, graph, monitor, token);
    Ok(Some(view))
}
