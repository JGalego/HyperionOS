use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};

use crate::types::{
    AuditAction, AuditLogEntry, AuditPayload, ObservabilityError, PrincipalRef, VerificationReport,
};

const GENESIS_HASH: u64 = 0;

fn compute_hash(
    prev_hash: u64,
    seq: u64,
    actor: PrincipalRef,
    action: AuditAction,
    target: &Option<String>,
    payload: &AuditPayload,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    prev_hash.hash(&mut hasher);
    seq.hash(&mut hasher);
    format!("{actor:?}").hash(&mut hasher);
    format!("{action:?}").hash(&mut hasher);
    target.hash(&mut hasher);
    // docs/34 §2: "entry_hash = H(prev_hash || canonical(payload) || seq)" —
    // this crate's `canonical(payload)` is the derived `Debug`
    // representation, deterministic given fixed field order, standing in
    // for a real canonical serialization.
    format!("{payload:?}").hash(&mut hasher);
    hasher.finish()
}

/// docs/34 §2/§6's Audit Ledger: "exactly one enforcement point" —
/// `append` is the only write path, tamper-evident via a hash chain, and
/// never rolled up, summarized, or deleted by the system.
pub struct AuditLedger {
    entries: Mutex<Vec<AuditLogEntry>>,
    next_seq: AtomicU64,
    last_hash: Mutex<u64>,
}

impl Default for AuditLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditLedger {
    pub fn new() -> Self {
        AuditLedger {
            entries: Mutex::new(Vec::new()),
            next_seq: AtomicU64::new(1),
            last_hash: Mutex::new(GENESIS_HASH),
        }
    }

    fn require(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        rights: RightsMask,
    ) -> Result<(), ObservabilityError> {
        monitor
            .check_rights_ok_result(token, rights)
            .map_err(|_| ObservabilityError::Unauthorized)
    }

    /// docs/34 §6's `Audit.append` — synchronous, durable, never dropped;
    /// the ledger's own capability check is this crate's fail-closed
    /// enforcement of "no Capability/Agent/Plugin holds direct write
    /// access."
    #[allow(clippy::too_many_arguments)]
    pub fn append(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        actor: PrincipalRef,
        action: AuditAction,
        target: Option<String>,
        payload: AuditPayload,
        now: u64,
    ) -> Result<AuditLogEntry, ObservabilityError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let mut last_hash = self.last_hash.lock().unwrap();
        let prev_hash = *last_hash;
        let entry_hash = compute_hash(prev_hash, seq, actor, action, &target, &payload);

        let entry = AuditLogEntry {
            seq,
            prev_hash,
            entry_hash,
            actor,
            action,
            target,
            payload,
            timestamp: now,
        };
        self.entries.lock().unwrap().push(entry.clone());
        *last_hash = entry_hash;
        Ok(entry)
    }

    /// docs/34 §6's `Audit.query` — local-only; capability-checking here
    /// mirrors `append`'s (a reader still needs a valid, live token, even
    /// though queries have no side effect).
    pub fn query(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        filter: impl Fn(&AuditLogEntry) -> bool,
    ) -> Result<Vec<AuditLogEntry>, ObservabilityError> {
        self.require(monitor, token, RightsMask::READ)?;
        Ok(self
            .entries
            .lock()
            .unwrap()
            .iter()
            .filter(|e| filter(e))
            .cloned()
            .collect())
    }

    /// docs/34 §6's `Audit.verifyChain` — recomputes every hash in
    /// `[from_seq, to_seq]` and checks both the hash-chain link and
    /// `seq` gaplessness; the first mismatch is reported, not silently
    /// repaired (docs/34 §5: "ledger break is never silently repaired").
    pub fn verify_chain(&self, from_seq: u64, to_seq: u64) -> VerificationReport {
        let entries = self.entries.lock().unwrap();
        let range: Vec<&AuditLogEntry> = entries
            .iter()
            .filter(|e| e.seq >= from_seq && e.seq <= to_seq)
            .collect();
        if range.is_empty() {
            return VerificationReport::Empty;
        }

        let mut expected_prev = if from_seq == 1 {
            GENESIS_HASH
        } else {
            range[0].prev_hash
        };
        for (expected_seq, entry) in (range[0].seq..).zip(range.iter()) {
            if entry.seq != expected_seq || entry.prev_hash != expected_prev {
                return VerificationReport::Corrupt { at_seq: entry.seq };
            }
            let recomputed = compute_hash(
                entry.prev_hash,
                entry.seq,
                entry.actor,
                entry.action,
                &entry.target,
                &entry.payload,
            );
            if recomputed != entry.entry_hash {
                return VerificationReport::Corrupt { at_seq: entry.seq };
            }
            expected_prev = entry.entry_hash;
        }
        VerificationReport::Intact
    }
}
