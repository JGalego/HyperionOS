//! docs/09-knowledge-graph.md §5.4: "edge deletions are tombstones carrying
//! a version vector so a deletion is never silently undone by a
//! late-arriving insertion from a device that hadn't seen it yet." This
//! crate simplifies the multi-replica version vector to a single monotonic
//! counter per triple (see crate doc), but the core invariant is the same
//! and is what these tests — including the property test — check.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{EdgeOrigin, KnowledgeGraph, LinkOutcome};
use proptest::prelude::*;
use serde_json::json;

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    (dir, monitor, token)
}

#[test]
fn blind_insert_after_tombstone_is_suppressed_not_resurrected() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let a = graph
        .put_node(&monitor, &token, None, "a", None, json!({}))
        .unwrap();
    let b = graph
        .put_node(&monitor, &token, None, "b", None, json!({}))
        .unwrap();

    let outcome = graph
        .link(
            &monitor,
            &token,
            a,
            "rel",
            b,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "test",
            None,
        )
        .unwrap();
    let edge_id = match outcome {
        LinkOutcome::Created(id) => id,
        other => panic!("expected Created, got {other:?}"),
    };

    graph.unlink(&monitor, &token, edge_id).unwrap();

    // A device that never saw the tombstone (observed_version = None) tries
    // to re-insert the same triple — it must not resurrect the edge.
    let outcome = graph
        .link(
            &monitor,
            &token,
            a,
            "rel",
            b,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "test",
            None,
        )
        .unwrap();
    assert!(matches!(outcome, LinkOutcome::SuppressedByTombstone(id) if id == edge_id));

    let explain = graph
        .explain(
            &monitor,
            &token,
            hyperion_knowledge_graph::ExplainRef::Edge(edge_id),
        )
        .unwrap();
    match explain {
        hyperion_knowledge_graph::ProvenanceChain::Edge { tombstone, .. } => {
            assert!(
                tombstone,
                "suppressed insert must not have un-tombstoned the edge"
            )
        }
        _ => panic!("expected an edge provenance chain"),
    }
}

#[test]
fn aware_insert_after_tombstone_recreates_the_edge() {
    let (dir, monitor, token) = setup();
    let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
    let a = graph
        .put_node(&monitor, &token, None, "a", None, json!({}))
        .unwrap();
    let b = graph
        .put_node(&monitor, &token, None, "b", None, json!({}))
        .unwrap();

    let outcome = graph
        .link(
            &monitor,
            &token,
            a,
            "rel",
            b,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "test",
            None,
        )
        .unwrap();
    let edge_id = match outcome {
        LinkOutcome::Created(id) => id,
        other => panic!("expected Created, got {other:?}"),
    };
    graph.unlink(&monitor, &token, edge_id).unwrap();

    // Version at time of tombstone is 1 (created at version 0, tombstoned
    // bumps to 1); a caller that observed that version explicitly re-links.
    let outcome = graph
        .link(
            &monitor,
            &token,
            a,
            "rel",
            b,
            1.0,
            EdgeOrigin::Explicit,
            None,
            "test",
            Some(1),
        )
        .unwrap();
    assert!(matches!(outcome, LinkOutcome::Created(id) if id == edge_id));
}

proptest! {
    #[test]
    fn tombstone_is_never_resurrected_by_a_blind_insert(
        ops in prop::collection::vec(prop::bool::ANY, 1..30)
    ) {
        let dir = tempfile::tempdir().unwrap();
        let mut monitor = CapabilityMonitor::new();
        let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
        let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();
        let a = graph.put_node(&monitor, &token, None, "a", None, json!({})).unwrap();
        let b = graph.put_node(&monitor, &token, None, "b", None, json!({})).unwrap();

        // Ground-truth model, advanced in lockstep with the real calls below,
        // so every outcome can be checked against what *should* have
        // happened rather than just asserting internal self-consistency.
        let mut tombstoned = false;
        let mut edge_exists = false;
        let mut last_edge_id = None;

        for op_is_link in ops {
            if op_is_link {
                // Always a *blind* insert: no replica ever tracks the version
                // it last observed, modeling the worst case for resurrection.
                let outcome = graph
                    .link(&monitor, &token, a, "rel", b, 1.0, EdgeOrigin::Explicit, None, "test", None)
                    .unwrap();
                if tombstoned {
                    prop_assert!(matches!(outcome, LinkOutcome::SuppressedByTombstone(_)));
                } else if edge_exists {
                    prop_assert!(matches!(outcome, LinkOutcome::Updated(_)));
                } else {
                    prop_assert!(matches!(outcome, LinkOutcome::Created(_)));
                }
                let id = match outcome {
                    LinkOutcome::Created(id) | LinkOutcome::Updated(id) | LinkOutcome::SuppressedByTombstone(id) => id,
                };
                last_edge_id = Some(id);
                edge_exists = true;
            } else if let Some(id) = last_edge_id {
                graph.unlink(&monitor, &token, id).unwrap();
                tombstoned = true;
            }
        }

        if let Some(id) = last_edge_id {
            let explain = graph
                .explain(&monitor, &token, hyperion_knowledge_graph::ExplainRef::Edge(id))
                .unwrap();
            let hyperion_knowledge_graph::ProvenanceChain::Edge { tombstone, .. } = explain else {
                panic!("expected an edge provenance chain");
            };
            prop_assert_eq!(tombstone, tombstoned, "on-disk tombstone state must match the model");
        }
    }
}
