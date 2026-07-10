use std::collections::HashMap;

use hyperion_storage::Wal;

use crate::types::{EdgeId, EdgeRecord, NodeId, NodeRecord, Record};

/// The rebuildable materialized view this crate maintains on top of
/// `hyperion-storage`'s WAL: node table, edge table, and the forward/
/// backward adjacency lists docs/29 §Algorithms' bidirectional traversal
/// query needs. Never the source of truth — [`GraphIndex::rebuild`] can
/// always reconstruct this from the WAL alone.
#[derive(Debug, Default)]
pub(crate) struct GraphIndex {
    pub(crate) nodes: HashMap<NodeId, NodeRecord>,
    pub(crate) edges: HashMap<EdgeId, EdgeRecord>,
    /// `subject -> edge ids where that node is the subject`.
    pub(crate) forward: HashMap<NodeId, Vec<EdgeId>>,
    /// `target -> edge ids where that node is the target`.
    pub(crate) backward: HashMap<NodeId, Vec<EdgeId>>,
    /// `(subject, predicate, target) -> edge id`, one entry per triple no
    /// matter how many times it's been updated/tombstoned/re-linked — the
    /// lookup [`crate::graph::KnowledgeGraph::link`] needs to find "is there
    /// already an edge for this triple" without a linear scan.
    pub(crate) edge_identity: HashMap<(NodeId, String, NodeId), EdgeId>,
}

impl GraphIndex {
    pub(crate) fn rebuild(path: &std::path::Path) -> Result<Self, hyperion_storage::StorageError> {
        let mut index = GraphIndex::default();
        for wal_record in Wal::replay(path)? {
            if let Ok(record) = serde_json::from_value::<Record>(wal_record.metadata) {
                index.apply(wal_record.object_id, record);
            }
        }
        Ok(index)
    }

    pub(crate) fn apply(&mut self, id: hyperion_storage::ObjectId, record: Record) {
        match record {
            Record::Node(node) => {
                self.nodes.insert(id, node);
            }
            Record::Edge(edge) => {
                let is_new = !self.edges.contains_key(&id);
                if is_new {
                    self.forward.entry(edge.subject).or_default().push(id);
                    self.backward.entry(edge.target).or_default().push(id);
                }
                self.edge_identity
                    .insert((edge.subject, edge.predicate.clone(), edge.target), id);
                self.edges.insert(id, edge);
            }
        }
    }

    pub(crate) fn active_edge(&self, id: EdgeId) -> Option<&EdgeRecord> {
        self.edges.get(&id).filter(|e| !e.tombstone)
    }
}
