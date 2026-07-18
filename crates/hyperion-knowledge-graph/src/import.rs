//! Real JSON graph import -- the counterpart to [`crate::export::to_json`], closing this crate's
//! own previously-unnamed "no pre-population/seed API" gap: a brand-new install's Knowledge Graph
//! starts genuinely empty, with no way to load a starter dataset (or restore a prior export)
//! short of replaying raw `put_node`/`link` calls by hand.
//!
//! Every imported node is created fresh, owned by the *importing caller's own* Trust Boundary --
//! an export's own recorded `owner`/`created_at`/`updated_at` fields are read back only for
//! display, never re-applied, so an import can never forge another boundary's ownership or
//! backdate a timestamp. Since two independent graphs (or an export file and the graph it's being
//! imported into) mint node ids from independent counters, imported ids are never reused directly
//! -- a real translation table (built while importing nodes, the same shape
//! `hyperion_federation::kg_sync::KgTranslation` already established for the identical problem)
//! maps the export's own ids to the real ids this graph actually assigns, and edges are
//! reconnected through it.

use std::collections::HashMap;

use hyperion_capability::{CapabilityMonitor, CapabilityToken};
use serde::Deserialize;
use serde_json::Value;

use crate::graph::KnowledgeGraph;
use crate::types::{EdgeOrigin, GraphError, ImportReport, NodeId};

#[derive(Deserialize)]
struct ImportedNode {
    id: u64,
    object_type: String,
    metadata: Value,
}

#[derive(Deserialize)]
struct ImportedEdge {
    subject: u64,
    predicate: String,
    target: u64,
    weight: f32,
    provenance: String,
    origin: EdgeOrigin,
    confidence: Option<f32>,
}

#[derive(Deserialize)]
struct GraphImport {
    nodes: Vec<ImportedNode>,
    edges: Vec<ImportedEdge>,
}

/// Real, capability-gated JSON import against an already-open graph -- see this module's own doc
/// comment for the ownership/id-translation rules. Malformed JSON is a real, named error
/// ([`GraphError::Malformed`]), never a partial import silently left half-applied: parsing
/// happens in full before any node is created.
pub fn import_json(
    graph: &KnowledgeGraph,
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    json: &str,
) -> Result<ImportReport, GraphError> {
    let parsed: GraphImport =
        serde_json::from_str(json).map_err(|e| GraphError::Malformed(e.to_string()))?;

    let mut translation: HashMap<u64, NodeId> = HashMap::with_capacity(parsed.nodes.len());
    for node in &parsed.nodes {
        let real_id = graph.put_node(
            monitor,
            token,
            None,
            node.object_type.clone(),
            None,
            node.metadata.clone(),
        )?;
        translation.insert(node.id, real_id);
    }

    let mut edges_created = 0;
    let mut edges_skipped_unresolved = 0;
    for edge in &parsed.edges {
        let (Some(&subject), Some(&target)) = (
            translation.get(&edge.subject),
            translation.get(&edge.target),
        ) else {
            edges_skipped_unresolved += 1;
            continue;
        };
        graph.link(
            monitor,
            token,
            subject,
            &edge.predicate,
            target,
            edge.weight,
            edge.origin,
            edge.confidence,
            &edge.provenance,
            None,
        )?;
        edges_created += 1;
    }

    Ok(ImportReport {
        nodes_created: parsed.nodes.len(),
        edges_created,
        edges_skipped_unresolved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_capability::{RightsMask, TrustBoundaryId};

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
    fn malformed_json_is_a_real_named_error_not_a_panic() {
        let (dir, monitor, token) = setup();
        let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

        let result = import_json(&graph, &monitor, &token, "not json");
        assert!(matches!(result, Err(GraphError::Malformed(_))));
    }

    #[test]
    fn an_empty_import_creates_nothing() {
        let (dir, monitor, token) = setup();
        let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

        let report =
            import_json(&graph, &monitor, &token, r#"{"nodes": [], "edges": []}"#).unwrap();
        assert_eq!(report.nodes_created, 0);
        assert_eq!(report.edges_created, 0);
        assert_eq!(report.edges_skipped_unresolved, 0);
    }

    #[test]
    fn a_real_export_round_trips_through_import_with_ids_translated() {
        let (dir, monitor, token) = setup();
        let source = KnowledgeGraph::open(dir.path().join("source.jsonl")).unwrap();
        let a = source
            .put_node(
                &monitor,
                &token,
                None,
                "document",
                None,
                serde_json::json!({"title": "seed doc"}),
            )
            .unwrap();
        let b = source
            .put_node(
                &monitor,
                &token,
                None,
                "document",
                None,
                serde_json::json!({}),
            )
            .unwrap();
        source
            .link(
                &monitor,
                &token,
                a,
                "relates_to",
                b,
                0.5,
                EdgeOrigin::Explicit,
                None,
                "seed",
                None,
            )
            .unwrap();
        let exported = source.export_json(&monitor, &token).unwrap();

        let dest = KnowledgeGraph::open(dir.path().join("dest.jsonl")).unwrap();
        // A decoy node first, so `dest`'s own id counter is offset from `source`'s -- otherwise
        // both freshly-opened graphs would coincidentally mint the same first id, and a real
        // "did the import genuinely translate rather than reuse the foreign id" bug would go
        // undetected (the same test-design fix `hyperion-federation::kg_sync`'s own tests needed).
        dest.put_node(&monitor, &token, None, "decoy", None, serde_json::json!({}))
            .unwrap();

        let report = import_json(&dest, &monitor, &token, &exported).unwrap();
        assert_eq!(report.nodes_created, 2);
        assert_eq!(report.edges_created, 1);
        assert_eq!(report.edges_skipped_unresolved, 0);

        let hits = dest
            .query(&monitor, &token, &crate::types::GraphQuery::default())
            .unwrap();
        let imported_doc = hits
            .iter()
            .find(|h| h.node.metadata.get("title").and_then(|v| v.as_str()) == Some("seed doc"))
            .expect("the imported node's own real metadata must survive the round trip");
        assert_ne!(
            imported_doc.node_id, a,
            "the imported node must get this graph's own real id, not reuse the foreign one"
        );
    }

    #[test]
    fn an_unresolved_edge_reference_is_skipped_not_a_crash() {
        let (dir, monitor, token) = setup();
        let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

        let json = r#"{
            "nodes": [{"id": 1, "object_type": "document", "metadata": {}, "owner": 1, "created_at": 0, "updated_at": 0}],
            "edges": [{"id": 1, "subject": 1, "predicate": "relates_to", "target": 9999, "weight": 1.0, "provenance": "test", "origin": "Explicit", "confidence": null, "owner": 1, "created_at": 0}]
        }"#;
        let report = import_json(&graph, &monitor, &token, json).unwrap();
        assert_eq!(report.nodes_created, 1);
        assert_eq!(report.edges_created, 0);
        assert_eq!(report.edges_skipped_unresolved, 1);
    }

    #[test]
    fn an_imported_node_is_owned_by_the_importing_callers_own_boundary_not_the_exports_claim() {
        let (dir, monitor, token) = setup();
        let graph = KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap();

        // The export claims owner 999 -- must never be honored.
        let json = r#"{
            "nodes": [{"id": 1, "object_type": "document", "metadata": {}, "owner": 999, "created_at": 0, "updated_at": 0}],
            "edges": []
        }"#;
        import_json(&graph, &monitor, &token, json).unwrap();

        let hits = graph
            .query(&monitor, &token, &crate::types::GraphQuery::default())
            .unwrap();
        assert_eq!(
            hits.len(),
            1,
            "the caller's own token must see the imported node"
        );
    }
}
