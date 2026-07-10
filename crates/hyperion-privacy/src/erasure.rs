use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_knowledge_graph::{KnowledgeGraph, NodeId};
use hyperion_recovery::{RecoveryService, Trigger};

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
///
/// `ErasureMode::SoftDelete` registers a real [33 — Rollback &
/// Recovery](../33-rollback-recovery.md) grace period: `recovery` opens a
/// recovery point capturing every object's pre-erasure state, journals
/// the erasure as a single committed `ActionRecord`, and returns its
/// `ActionId` on the receipt so a caller can later reverse the erasure
/// with `recovery.undo(UndoScope::SingleAction(action_id))`.
/// `ErasureMode::CryptoShred` is deliberately excluded from this — it's
/// this crate's no-grace-period, no-recovery path (see [`ErasureMode`]'s
/// own doc comment), so nothing is journaled and the receipt's
/// `grace_period_action` is `None`.
pub fn erase(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    graph: &KnowledgeGraph,
    recovery: &RecoveryService,
    object_ids: &[NodeId],
    mode: ErasureMode,
    now: u64,
) -> Result<ErasureReceipt, PrivacyError> {
    require(monitor, token, RightsMask::WRITE)?;

    let grace_period_action = match mode {
        ErasureMode::SoftDelete => {
            let point_id = recovery.recovery_point_create(
                monitor,
                token,
                Trigger::UserRequested,
                object_ids,
                now,
            )?;
            Some(recovery.record_action_started(
                point_id,
                object_ids.to_vec(),
                None,
                "hyperion-privacy erase (soft-delete)",
                now,
            ))
        }
        ErasureMode::CryptoShred => None,
    };

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

    if let Some(action_id) = grace_period_action {
        recovery.record_action_committed(action_id)?;
    }

    Ok(ErasureReceipt {
        object_ids: object_ids.to_vec(),
        mode,
        completed_at: Some(now),
        grace_period_action,
    })
}
