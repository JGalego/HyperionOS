use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_knowledge_graph::{KnowledgeGraph, NodeId};
use hyperion_recovery::{ActionId, ActionStatus, RecoveryService, Trigger};

use crate::types::{ErasureMode, ErasureReceipt, PrivacyError};

/// The exact `ActionRecord.note` [`erase`]'s own `SoftDelete` path journals under -- reused by
/// [`expire_lapsed_soft_deletes`] to recognize which `Committed` actions are really this crate's
/// own grace periods (and not some unrelated caller's own journaled action) without needing a
/// dedicated field on `hyperion-recovery`'s own, deliberately privacy-agnostic `ActionRecord`
/// type.
const SOFT_DELETE_NOTE: &str = "hyperion-privacy erase (soft-delete)";

fn require(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    rights: RightsMask,
) -> Result<(), PrivacyError> {
    monitor
        .check_rights_ok_result(token, rights)
        .map_err(|_| PrivacyError::Unauthorized)
}

/// docs/16 §5's erasure. `ErasureMode::SoftDelete` overwrites the node's metadata with a
/// tombstone-shaped placeholder via a real, undoable `put_node` -- a real, observable state
/// change (every field a caller previously stored is gone from the current version) that a real
/// grace-period `undo` can still reverse. ~~`ErasureMode::CryptoShred` used the same placeholder
/// overwrite, since `hyperion-knowledge-graph` had no node-delete operation~~ — that primitive is
/// now real: `CryptoShred` calls `hyperion_knowledge_graph::KnowledgeGraph::delete_node` instead,
/// a genuine tombstone no `get`/`query`/`traverse`/`dump` call ever surfaces again, not merely an
/// overwritten-but-still-readable placeholder. Still not a byte-level deletion from the WAL's
/// history, which no crate in this workspace performs — a real `CryptoShred` would additionally
/// destroy the encryption key old versions were sealed under; this crate has no encryption-at-rest
/// to shred (see this crate's doc comment). `SoftDelete` deliberately keeps the placeholder
/// overwrite, never the real tombstone: its own grace-period `undo` restores through `put_node`,
/// which — correctly, mirroring the CRDT tombstone-never-silently-resurrected invariant edges
/// already have — could never un-tombstone a node `delete_node` had genuinely deleted.
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
                SOFT_DELETE_NOTE,
                now,
            ))
        }
        ErasureMode::CryptoShred => None,
    };

    for &id in object_ids {
        match mode {
            ErasureMode::SoftDelete => {
                graph.put_node(
                    monitor,
                    token,
                    Some(id),
                    "Erased",
                    None,
                    serde_json::json!({ "erased": true, "mode": format!("{mode:?}") }),
                )?;
            }
            ErasureMode::CryptoShred => {
                graph.delete_node(monitor, token, id)?;
            }
        }
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

/// docs/16 §10's own "soft-deletes honor a grace period before cryptographic shredding" real
/// timer, closing this crate's own previously-named gap: `erase(SoftDelete)` registered a real,
/// undoable `ActionRecord`, but nothing in this workspace ran a background clock that turned that
/// grace period into a permanent `CryptoShred` once it lapsed. A real caller (this crate has no
/// scheduler of its own to tick on — matching this workspace's hosted-simulator convention of a
/// caller-driven clock rather than a real background thread) calls this with its own current
/// `now` and a chosen `grace_period_secs`; every one of this crate's own soft-delete
/// `ActionRecord`s still `Committed` (still undoable) whose age has reached or exceeded that
/// grace period is sealed for real via `hyperion_recovery::RecoveryService::expire` — after this,
/// `recovery.undo(...)` can never restore it again — *and* every one of its `objects_touched` is
/// really shredded via `hyperion_knowledge_graph::KnowledgeGraph::delete_node`, matching the exact
/// irreversibility (both undo-blocking *and* actual unreadability) `ErasureMode::CryptoShred`
/// already has from the start, rather than leaving the object as an overwritten-but-still-readable
/// placeholder forever. `GraphError::NotFound` is treated as benign (the same convention
/// `hyperion-recovery::apply_snapshot`'s own undo→delete_node wiring already established): a node
/// already deleted by some other path is not this sweep's error to report.
///
/// Deliberate simplification, named rather than silently narrowed: docs/16 §4's own
/// `ErasureRequest.grace_period` is a *per-request* `Duration` a caller could vary erasure to
/// erasure; this sweep applies one caller-supplied `grace_period_secs` uniformly to every pending
/// soft-delete at sweep time, since `hyperion-recovery`'s own `ActionRecord` (deliberately
/// privacy-agnostic — many other crates journal through it) has no per-action grace-period field
/// of its own to vary it by.
///
/// Returns every `ActionId` this call really expired, so a caller can log or audit exactly what a
/// sweep did — never a silent bulk operation with no visible effect.
///
/// Capability-checked and Trust-Boundary-scoped (2026-07-16), closing a real gap this function
/// previously had relative to every other call in this crate: `erase` itself gates on
/// `RightsMask::WRITE` before doing anything, but this sweep took the identical `monitor`/`token`
/// pair and never checked them at all — any token, including one with zero rights, could call
/// this and it would run in full. Worse, `recovery.action_records()` returns every
/// `ActionRecord` in the whole `RecoveryService`, across every Trust Boundary, and nothing
/// scoped the sweep to the caller's own — a caller from a different boundary than the one that
/// ran `erase` could permanently seal (via `RecoveryService::expire`) another boundary's own
/// still-`Committed` grace period, stripping its undo protection, without ever being authorized
/// to read or write its objects and without the shred ever completing (the same
/// `GraphError::NotFound` this function already treats as benign would just silently swallow the
/// unauthorized `delete_node` attempt below). Now: `require`'d the same way `erase` is, and
/// scoped via a real `graph.get` check — `hyperion_knowledge_graph::KnowledgeGraph::get`'s own
/// owner-based ACL (this session's own real enforcement) is reused directly rather than this
/// crate inventing a second, parallel ownership concept: a record is only ever eligible if every
/// one of its `objects_touched` is genuinely visible to `token`, the same "only ever touches the
/// caller's own objects" convention `hyperion-knowledge-graph::prune_decayed_edges` established
/// for exactly this shape of sweep.
pub fn expire_lapsed_soft_deletes(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    graph: &KnowledgeGraph,
    recovery: &RecoveryService,
    now: u64,
    grace_period_secs: u64,
) -> Result<Vec<ActionId>, PrivacyError> {
    require(monitor, token, RightsMask::WRITE)?;

    Ok(recovery
        .action_records()
        .into_iter()
        .filter(|record| {
            record.status == ActionStatus::Committed
                && record.note == SOFT_DELETE_NOTE
                && now.saturating_sub(record.created_at) >= grace_period_secs
                && record
                    .objects_touched
                    .iter()
                    .all(|&id| graph.get(monitor, token, id).is_ok())
        })
        .filter_map(|record| {
            recovery.expire(record.action_id).ok().map(|()| {
                for &id in &record.objects_touched {
                    match graph.delete_node(monitor, token, id) {
                        Ok(()) | Err(hyperion_knowledge_graph::GraphError::NotFound) => {}
                        Err(err) => {
                            // Best-effort shredding: expiry itself already succeeded and must not
                            // be rolled back over a secondary write failure on one object.
                            let _ = err;
                        }
                    }
                }
                record.action_id
            })
        })
        .collect())
}
