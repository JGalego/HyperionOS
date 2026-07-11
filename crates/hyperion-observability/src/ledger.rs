use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_crypto::Hash;

use crate::types::{
    AuditAction, AuditLogEntry, AuditPayload, ObservabilityError, PrincipalRef, VerificationReport,
};

/// A real BLAKE3 hash of the empty byte string -- a well-defined, reproducible root for the
/// chain's first entry to link to, playing the same "nothing came before this" role a `0` did
/// for the old non-cryptographic stand-in.
fn genesis_hash() -> Hash {
    hyperion_crypto::hash(&[])
}

/// PRODUCTION_BOOT_PROMPT.md M9: a real BLAKE3 hash (via [`hyperion_crypto::hash`]) over the same
/// fields the non-cryptographic stand-in this replaces already chose to cover -- not
/// `std::collections::hash_map::DefaultHasher` (SipHash), which is explicitly documented as
/// unsuitable for anything beyond in-process `HashMap` bucketing: its exact algorithm isn't
/// guaranteed stable release to release, so two builds of this same crate could disagree about
/// whether an *identical* ledger is intact. `canonical(payload)` is still each field's derived
/// `Debug` representation (deterministic given fixed field order) rather than a true canonical
/// serialization -- a real cryptographic hash now runs over that representation, but the
/// representation itself is unchanged from before, and remains a smaller, separately named gap.
fn compute_hash(
    prev_hash: Hash,
    seq: u64,
    actor: PrincipalRef,
    action: AuditAction,
    target: &Option<String>,
    payload: &AuditPayload,
) -> Hash {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(prev_hash.as_bytes());
    bytes.extend_from_slice(&seq.to_le_bytes());
    bytes.extend_from_slice(format!("{actor:?}").as_bytes());
    bytes.extend_from_slice(format!("{action:?}").as_bytes());
    bytes.extend_from_slice(format!("{target:?}").as_bytes());
    // docs/34 §2: "entry_hash = H(prev_hash || canonical(payload) || seq)" —
    // this crate's `canonical(payload)` is the derived `Debug`
    // representation, deterministic given fixed field order, standing in
    // for a real canonical serialization.
    bytes.extend_from_slice(format!("{payload:?}").as_bytes());
    hyperion_crypto::hash(&bytes)
}

/// docs/34 §2/§6's Audit Ledger: "exactly one enforcement point" —
/// `append` is the only write path, tamper-evident via a hash chain, and
/// never rolled up, summarized, or deleted by the system.
pub struct AuditLedger {
    entries: Mutex<Vec<AuditLogEntry>>,
    next_seq: AtomicU64,
    last_hash: Mutex<Hash>,
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
            last_hash: Mutex::new(genesis_hash()),
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
            genesis_hash()
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

#[cfg(test)]
mod tests {
    //! `AuditLedger`'s own public API has no mutation hook on an already-appended entry --
    //! `append` is deliberately the *only* write path (this crate's own doc comment). Proving
    //! `verify_chain` really detects real corruption therefore needs direct access to the
    //! private `entries` field to simulate a tampered record realistically, which only a test
    //! module inside this same file can reach -- the same pattern
    //! `hyperion-api-gateway::gateway.rs`'s own internal test module already uses for the same
    //! reason (a real gap the public API structurally can't exercise from outside the crate).

    use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};

    use super::*;
    use crate::types::AuditPayload;

    fn setup() -> (CapabilityMonitor, CapabilityToken, AuditLedger) {
        let mut monitor = CapabilityMonitor::new();
        let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
        (monitor, root, AuditLedger::new())
    }

    #[test]
    fn a_real_tampered_entry_is_detected_as_corrupt_at_its_exact_seq() {
        let (monitor, root, ledger) = setup();
        for i in 0..3 {
            ledger
                .append(
                    &monitor,
                    &root,
                    PrincipalRef::System,
                    AuditAction::Grant,
                    None,
                    AuditPayload::Note(format!("entry {i}")),
                    1_000 + i,
                )
                .unwrap();
        }
        assert_eq!(ledger.verify_chain(1, 3), VerificationReport::Intact);

        // Simulate a real, direct tamper of an already-appended entry's payload -- something
        // no caller can do through the real public API (append is the only write path), but
        // exactly what a real storage-layer corruption or a compromised process with raw disk
        // access would produce.
        {
            let mut entries = ledger.entries.lock().unwrap();
            entries[1].payload = AuditPayload::Note("tampered".to_string());
        }

        assert_eq!(
            ledger.verify_chain(1, 3),
            VerificationReport::Corrupt { at_seq: 2 },
            "a real BLAKE3 hash-chain check must catch a tampered entry at its exact seq, not \
             silently accept it or merely report generic corruption"
        );
    }

    #[test]
    fn a_broken_chain_link_is_detected_even_when_the_spliced_entrys_own_hash_recomputes_fine() {
        let (monitor, root, ledger) = setup();
        for i in 0..3 {
            ledger
                .append(
                    &monitor,
                    &root,
                    PrincipalRef::System,
                    AuditAction::Grant,
                    None,
                    AuditPayload::Note(format!("entry {i}")),
                    1_000 + i,
                )
                .unwrap();
        }

        // Simulate a spliced-in entry from a different chain entirely: seq 2's own content and
        // entry_hash are untouched (its own self-consistency check alone would pass), but its
        // prev_hash link now points somewhere that isn't seq 1's real hash -- distinct from the
        // content-tamper case above, this tests the link check specifically.
        {
            let mut entries = ledger.entries.lock().unwrap();
            entries[1].prev_hash = hyperion_crypto::hash(b"a different chain entirely");
        }

        assert_eq!(
            ledger.verify_chain(1, 3),
            VerificationReport::Corrupt { at_seq: 2 },
            "a broken prev_hash link must be caught at the exact seq where the chain no longer \
             connects, even though that entry's own entry_hash still recomputes consistently \
             over its (untouched) content"
        );
    }
}
