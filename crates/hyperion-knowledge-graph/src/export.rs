//! Real JSON graph export -- this crate's own previously-unnamed gap: [`crate::types::
//! GraphSnapshot`] already derives `Serialize` (added for `hyperion_federation::kg_sync`'s own
//! wire format), but nothing ever exposed a caller-facing "give me this graph as JSON" API, and a
//! bare `serde_json::to_string(&snapshot)` would produce an awkward interchange shape anyway --
//! `Vec<(NodeId, NodeRecord)>` serializes as an array of two-element arrays (`[[5, {...}], ...]`),
//! not the array-of-objects-with-an-`id`-field shape any real external tool (or a person reading
//! it) would expect. [`to_json`] reshapes the same real, already-visibility-filtered snapshot
//! [`crate::graph::KnowledgeGraph::dump`] produces into exactly that shape instead.

use serde::Serialize;
use serde_json::Value;

use crate::types::{EdgeOrigin, GraphSnapshot};

#[derive(Serialize)]
struct ExportedNode {
    id: u64,
    object_type: String,
    metadata: Value,
    owner: u64,
    device_origin: u64,
    created_at: u64,
    updated_at: u64,
}

#[derive(Serialize)]
struct ExportedEdge {
    id: u64,
    subject: u64,
    predicate: String,
    target: u64,
    weight: f32,
    provenance: String,
    origin: EdgeOrigin,
    confidence: Option<f32>,
    owner: u64,
    created_at: u64,
}

#[derive(Serialize)]
struct GraphExport {
    nodes: Vec<ExportedNode>,
    edges: Vec<ExportedEdge>,
}

/// Reshapes one [`GraphSnapshot`] into a pretty-printed, self-contained JSON document -- every
/// node and edge id inlined as a plain field rather than left as tuple-positional, so the result
/// is meaningful to a reader or tool with no knowledge of this crate's own internal
/// `Vec<(id, record)>` shape. Never fails: every field here is already-validated, in-memory data
/// (`serde_json`'s own serialization can only fail on non-finite floats or non-string map keys,
/// neither of which this crate's own types ever produce), so this returns a plain `String` rather
/// than a `Result`.
pub fn to_json(snapshot: &GraphSnapshot) -> String {
    let export = GraphExport {
        nodes: snapshot
            .nodes
            .iter()
            .map(|(id, node)| ExportedNode {
                id: id.0,
                object_type: node.object_type.clone(),
                metadata: node.metadata.clone(),
                owner: node.owner,
                device_origin: node.device_origin,
                created_at: node.created_at,
                updated_at: node.updated_at,
            })
            .collect(),
        edges: snapshot
            .edges
            .iter()
            .map(|(id, edge)| ExportedEdge {
                id: id.0,
                subject: edge.subject.0,
                predicate: edge.predicate.clone(),
                target: edge.target.0,
                weight: edge.weight,
                provenance: edge.provenance.clone(),
                origin: edge.origin,
                confidence: edge.confidence,
                owner: edge.owner,
                created_at: edge.created_at,
            })
            .collect(),
    };
    serde_json::to_string_pretty(&export).expect("GraphExport is always representable as JSON")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{EdgeRecord, NodeRecord};
    use hyperion_storage::ObjectId;

    #[test]
    fn an_empty_snapshot_exports_to_empty_arrays() {
        let snapshot = GraphSnapshot {
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        let json = to_json(&snapshot);
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["nodes"].as_array().unwrap().len(), 0);
        assert_eq!(parsed["edges"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn a_node_and_edge_export_with_their_id_inlined_as_a_plain_field() {
        let node = NodeRecord {
            object_type: "document".to_string(),
            embedding: None,
            metadata: serde_json::json!({"title": "a real doc"}),
            owner: 1,
            device_origin: 42,
            origin: crate::types::NodeOrigin::default(),
            corroboration_count: 0,
            created_at: 100,
            updated_at: 100,
            tombstone: false,
        };
        let edge = EdgeRecord {
            subject: ObjectId(5),
            predicate: "relates_to".to_string(),
            target: ObjectId(6),
            weight: 0.8,
            provenance: "user_explicit".to_string(),
            origin: EdgeOrigin::Explicit,
            confidence: Some(0.9),
            owner: 1,
            created_at: 100,
            last_confirmed_at: 100,
            tombstone: false,
            version: 1,
        };
        let snapshot = GraphSnapshot {
            nodes: vec![(ObjectId(5), node)],
            edges: vec![(ObjectId(7), edge)],
        };

        let json = to_json(&snapshot);
        let parsed: Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["nodes"][0]["id"], 5);
        assert_eq!(parsed["nodes"][0]["object_type"], "document");
        assert_eq!(parsed["nodes"][0]["metadata"]["title"], "a real doc");
        assert_eq!(parsed["nodes"][0]["device_origin"], 42);

        assert_eq!(parsed["edges"][0]["id"], 7);
        assert_eq!(parsed["edges"][0]["subject"], 5);
        assert_eq!(parsed["edges"][0]["target"], 6);
        assert_eq!(parsed["edges"][0]["predicate"], "relates_to");
        assert_eq!(parsed["edges"][0]["confidence"], 0.9);
    }
}
