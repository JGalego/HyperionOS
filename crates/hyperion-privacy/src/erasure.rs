use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_knowledge_graph::{KnowledgeGraph, NodeId};

use crate::types::{ErasureMode, ErasureReceipt, PrivacyError};

fn require(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    rights: RightsMask,
) -> Result<(), PrivacyError> {
    monitor
        .check_rights_ok_result(token, rights)
        .map_err(|_| PrivacyError::Unauthorized)
}

/// docs/16 §5's erasure: `hyperion-knowledge-graph` has no node-delete
/// operation (only edges tombstone — see this crate's doc comment), so
/// "erase" here overwrites the node's metadata with a tombstone-shaped
/// placeholder rather than physically removing it. This is a real,
/// observable state change (every field a caller previously stored is
/// gone from the current version), just not a byte-level deletion from
/// the WAL's history, which no crate in this workspace performs — a real
/// `CryptoShred` would additionally destroy the encryption key old
/// versions were sealed under; this crate has no encryption-at-rest to
/// shred (see this crate's doc comment).
pub fn erase(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    graph: &KnowledgeGraph,
    object_ids: &[NodeId],
    mode: ErasureMode,
    now: u64,
) -> Result<ErasureReceipt, PrivacyError> {
    require(monitor, token, RightsMask::WRITE)?;

    for &id in object_ids {
        graph.put_node(
            monitor,
            token,
            Some(id),
            "Erased",
            None,
            serde_json::json!({ "erased": true, "mode": format!("{mode:?}") }),
        )?;
    }

    Ok(ErasureReceipt {
        object_ids: object_ids.to_vec(),
        mode,
        completed_at: Some(now),
    })
}
