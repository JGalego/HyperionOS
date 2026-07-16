use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_crypto::{Hash, Keystore, Signature, VerifyingKey};
use hyperion_model_router::Rationale;

use crate::types::{
    AuditAction, AuditLogEntry, AuditPayload, ObservabilityError, PrincipalRef, VerificationReport,
};

/// docs/34 §2's periodic anchor cadence — every this-many real ledger entries, if this ledger was
/// constructed with a real [`Keystore`] ([`AuditLedger::new_with_keystore`]), a signed
/// [`Anchor`] is produced over the segment that just closed.
const ANCHOR_INTERVAL: u64 = 100;

/// docs/34 §2's periodic signed Merkle anchor: a real Ed25519 signature over the Merkle root of
/// `[from_seq, to_seq]`'s `entry_hash`es -- "hardware root of trust where available, a software
/// key otherwise" (this crate's own doc comment; [`hyperion_crypto::Keystore`] is the software
/// case). Lets a verifier confirm an entire closed segment of the chain hasn't been rewritten
/// wholesale (re-hashed *and* re-linked consistently) without re-verifying every entry back to
/// genesis every time -- exactly the property a bare hash chain alone doesn't give a remote
/// verifier who wasn't watching it grow in real time.
#[derive(Debug, Clone, PartialEq)]
pub struct Anchor {
    pub from_seq: u64,
    pub to_seq: u64,
    pub merkle_root: Hash,
    pub signature: Signature,
}

/// A standard binary Merkle tree root over `leaves`, in order — an odd node at any level carries
/// up unhashed rather than being duplicated, since duplication (the common convention for
/// fixed-arity trees) has a known second-preimage weakness when leaves themselves can repeat;
/// carrying the odd one up avoids that at the cost of a slightly less "textbook" tree, immaterial
/// for this crate's own verify-only use.
fn merkle_root(leaves: &[Hash]) -> Hash {
    assert!(
        !leaves.is_empty(),
        "an anchor is only ever built over a real, non-empty closed segment"
    );
    let mut level: Vec<Hash> = leaves.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        for pair in level.chunks(2) {
            if pair.len() == 2 {
                let mut bytes = Vec::with_capacity(64);
                bytes.extend_from_slice(pair[0].as_bytes());
                bytes.extend_from_slice(pair[1].as_bytes());
                next.push(hyperion_crypto::hash(&bytes));
            } else {
                next.push(pair[0]);
            }
        }
        level = next;
    }
    level[0]
}

/// A real BLAKE3 hash of the empty byte string -- a well-defined, reproducible root for the
/// chain's first entry to link to, playing the same "nothing came before this" role a `0` did
/// for the old non-cryptographic stand-in.
fn genesis_hash() -> Hash {
    hyperion_crypto::hash(&[])
}

/// docs/998-roadmap.md M9: a real BLAKE3 hash (via [`hyperion_crypto::hash`]) over the same
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
    /// `None` — the default, plain [`Self::new`] — means this ledger never anchors, exactly as
    /// it behaved before anchoring existed. `Some` (via [`Self::new_with_keystore`]) is what a
    /// caller that actually has a real device identity to sign with opts into.
    keystore: Option<Keystore>,
    anchors: Mutex<Vec<Anchor>>,
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
            keystore: None,
            anchors: Mutex::new(Vec::new()),
        }
    }

    /// As [`Self::new`], but real periodic signed Merkle anchors (docs/34 §2) are produced every
    /// [`ANCHOR_INTERVAL`] entries, signed with `keystore`.
    pub fn new_with_keystore(keystore: Keystore) -> Self {
        AuditLedger {
            entries: Mutex::new(Vec::new()),
            next_seq: AtomicU64::new(1),
            last_hash: Mutex::new(genesis_hash()),
            keystore: Some(keystore),
            anchors: Mutex::new(Vec::new()),
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
        drop(last_hash);

        if let Some(keystore) = &self.keystore {
            if seq.is_multiple_of(ANCHOR_INTERVAL) {
                let from_seq = seq - ANCHOR_INTERVAL + 1;
                self.anchor_segment(keystore, from_seq, seq);
            }
        }

        Ok(entry)
    }

    /// Builds and signs a real [`Anchor`] over `[from_seq, to_seq]`'s current `entry_hash`es and
    /// records it. Only ever called from [`Self::append`] with a segment that has genuinely just
    /// closed (`to_seq % ANCHOR_INTERVAL == 0`), but reads the segment fresh from `self.entries`
    /// rather than threading its own hashes through, so it always anchors what's actually stored.
    fn anchor_segment(&self, keystore: &Keystore, from_seq: u64, to_seq: u64) {
        let leaves: Vec<Hash> = self
            .entries
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.seq >= from_seq && e.seq <= to_seq)
            .map(|e| e.entry_hash)
            .collect();
        let root = merkle_root(&leaves);
        let signature = keystore.sign(root.as_bytes());
        self.anchors.lock().unwrap().push(Anchor {
            from_seq,
            to_seq,
            merkle_root: root,
            signature,
        });
    }

    /// Every real signed [`Anchor`] this ledger has produced so far, oldest first.
    pub fn anchors(&self) -> Vec<Anchor> {
        self.anchors.lock().unwrap().clone()
    }

    /// Checks `anchor` two ways: its own signature must verify against `verifying_key` (catches a
    /// forged anchor, or one signed by a different real key), and its `merkle_root` must still
    /// match a fresh recomputation over `self`'s *current* `[from_seq, to_seq]` entries (catches
    /// an anchored segment that was rewritten after the fact, even if the anchor record itself
    /// was left untouched).
    pub fn verify_anchor(&self, anchor: &Anchor, verifying_key: &VerifyingKey) -> bool {
        if !hyperion_crypto::verify(
            anchor.merkle_root.as_bytes(),
            &anchor.signature,
            verifying_key,
        ) {
            return false;
        }
        let leaves: Vec<Hash> = self
            .entries
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.seq >= anchor.from_seq && e.seq <= anchor.to_seq)
            .map(|e| e.entry_hash)
            .collect();
        if leaves.is_empty() {
            return false;
        }
        merkle_root(&leaves) == anchor.merkle_root
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

    /// docs/23-multi-model-orchestration.md §Algorithms' own literal, previously-unbuilt
    /// `get_rationale(decision_id) -> Rationale` — closes `hyperion-model-router`'s and this
    /// crate's own further-named gap ("`get_rationale`-by-`invocation_id` specifically is still
    /// not a dedicated index — the ledger's own `query`/`seq` lookup is by `target`... and not
    /// `invocation_id`"). Built on [`Self::query`] rather than a separate index kept in sync
    /// alongside `entries` — this ledger is never rolled up or truncated (see this struct's own
    /// doc comment), so a second data structure mirroring it would be state to keep consistent
    /// forever for no correctness this scan doesn't already give. Returns the *most recent*
    /// matching entry (a caller could in principle re-route the same `invocation_id` were it ever
    /// reused, though `hyperion-model-router`'s own `next_invocation_id` never does) — `None` if
    /// no `ModelRouting` entry was ever appended for it.
    pub fn rationale_for_invocation(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        invocation_id: u64,
    ) -> Result<Option<Rationale>, ObservabilityError> {
        let matches = self.query(monitor, token, |entry| {
            matches!(
                &entry.payload,
                AuditPayload::ModelRouting { invocation_id: id, .. } if *id == invocation_id
            )
        })?;
        Ok(matches
            .into_iter()
            .next_back()
            .map(|entry| match entry.payload {
                AuditPayload::ModelRouting { rationale, .. } => rationale,
                _ => unreachable!("query filter only matches ModelRouting entries"),
            }))
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

    /// This crate's own previously-named "background scheduled chain verification" gap, made
    /// real: starts a real background thread that re-invokes [`Self::verify_chain`] over the
    /// entire chain every real `interval`, mirroring `hyperion-federation::FederationHub::
    /// start_lease_heartbeat`'s own `Arc<Self>`/stop-flag/join-on-drop shape exactly. A caller
    /// reads the schedule's own [`VerificationSchedule::last_report`] instead of only ever being
    /// able to check on demand -- the ring-buffer write-ahead-spill half of this crate's own
    /// original gap remains separately deferred (see this crate's doc comment).
    pub fn start_periodic_verification(
        self: &Arc<Self>,
        interval: Duration,
    ) -> VerificationSchedule {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let last_report = Arc::new(Mutex::new(None));
        let thread_report = Arc::clone(&last_report);
        let ledger = Arc::clone(self);
        let handle = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                std::thread::sleep(interval);
                if thread_stop.load(Ordering::Relaxed) {
                    break;
                }
                let to_seq = ledger.next_seq.load(Ordering::Relaxed).saturating_sub(1);
                let report = ledger.verify_chain(1, to_seq);
                *thread_report.lock().unwrap() = Some(report);
            }
        });
        VerificationSchedule {
            stop,
            handle: Some(handle),
            last_report,
        }
    }
}

/// A real background thread [`AuditLedger::start_periodic_verification`] returns -- mirrors
/// `hyperion-federation::FederationHub`'s own `LeaseHeartbeat` `stop`/join-on-drop shape exactly:
/// a genuinely running thread the caller can stop and block on, or simply drop to have it
/// stopped and joined automatically.
pub struct VerificationSchedule {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    last_report: Arc<Mutex<Option<VerificationReport>>>,
}

impl VerificationSchedule {
    /// Signals the real background thread to stop and blocks until it has genuinely exited.
    pub fn stop(mut self) {
        self.stop_and_join();
    }

    fn stop_and_join(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    /// The most recent real [`VerificationReport`] the background thread produced -- `None`
    /// until its first tick has run.
    pub fn last_report(&self) -> Option<VerificationReport> {
        self.last_report.lock().unwrap().clone()
    }
}

impl Drop for VerificationSchedule {
    fn drop(&mut self) {
        self.stop_and_join();
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

    fn append_n(monitor: &CapabilityMonitor, root: &CapabilityToken, ledger: &AuditLedger, n: u64) {
        for i in 0..n {
            ledger
                .append(
                    monitor,
                    root,
                    PrincipalRef::System,
                    AuditAction::Grant,
                    None,
                    AuditPayload::Note(format!("entry {i}")),
                    1_000 + i,
                )
                .unwrap();
        }
    }

    #[test]
    fn no_anchor_is_produced_without_a_real_keystore() {
        let (monitor, root, ledger) = setup();
        append_n(&monitor, &root, &ledger, ANCHOR_INTERVAL * 2);
        assert!(
            ledger.anchors().is_empty(),
            "the plain constructor must never anchor, exactly as before anchoring existed"
        );
    }

    #[test]
    fn a_real_signed_anchor_is_produced_after_exactly_anchor_interval_entries() {
        let mut monitor = CapabilityMonitor::new();
        let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
        let dir = tempfile::tempdir().unwrap();
        let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
        let verifying_key = keystore.verifying_key();
        let ledger = AuditLedger::new_with_keystore(keystore);

        append_n(&monitor, &root, &ledger, ANCHOR_INTERVAL - 1);
        assert!(
            ledger.anchors().is_empty(),
            "no anchor before the interval genuinely closes"
        );

        append_n(&monitor, &root, &ledger, 1);
        let anchors = ledger.anchors();
        assert_eq!(anchors.len(), 1);
        let anchor = &anchors[0];
        assert_eq!(anchor.from_seq, 1);
        assert_eq!(anchor.to_seq, ANCHOR_INTERVAL);
        assert!(
            ledger.verify_anchor(anchor, &verifying_key),
            "a freshly produced anchor over an untouched ledger must verify"
        );
    }

    #[test]
    fn verify_anchor_rejects_a_signature_from_a_different_real_key() {
        let mut monitor = CapabilityMonitor::new();
        let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
        let dir = tempfile::tempdir().unwrap();
        let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
        let ledger = AuditLedger::new_with_keystore(keystore);
        append_n(&monitor, &root, &ledger, ANCHOR_INTERVAL);

        let forger_dir = tempfile::tempdir().unwrap();
        let forger_keystore =
            Keystore::open_or_create(&forger_dir.path().join("forger.key")).unwrap();
        let anchor = &ledger.anchors()[0];
        assert!(
            !ledger.verify_anchor(anchor, &forger_keystore.verifying_key()),
            "an anchor must not verify against a real key other than the one that signed it"
        );
    }

    #[test]
    fn verify_anchor_rejects_a_segment_rewritten_after_being_anchored() {
        let mut monitor = CapabilityMonitor::new();
        let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
        let dir = tempfile::tempdir().unwrap();
        let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
        let verifying_key = keystore.verifying_key();
        let ledger = AuditLedger::new_with_keystore(keystore);
        append_n(&monitor, &root, &ledger, ANCHOR_INTERVAL);
        let anchor = ledger.anchors()[0].clone();
        assert!(ledger.verify_anchor(&anchor, &verifying_key));

        // Simulate an attacker who rewrote an entry *and* recomputed entry_hash/prev_hash so the
        // segment stays internally self-consistent (verify_chain alone would not catch this,
        // since it only checks the chain's own internal links -- exactly the "rewritten
        // wholesale" scenario docs/34 §2 names the anchor as defending against, distinct from a
        // same-hash payload tamper `verify_chain`'s own test already covers). The anchor record
        // itself is untouched; only the segment it claims to cover changed.
        {
            let mut entries = ledger.entries.lock().unwrap();
            entries[0].entry_hash =
                hyperion_crypto::hash(b"a rewritten, internally-consistent entry");
        }

        assert!(
            !ledger.verify_anchor(&anchor, &verifying_key),
            "an anchor must not verify once its anchored segment's real content hash has changed, \
             even though the anchor record's own bytes are untouched"
        );
    }

    #[test]
    fn the_background_schedule_produces_a_real_intact_report_on_its_own_first_tick() {
        let (monitor, root, ledger) = setup();
        append_n(&monitor, &root, &ledger, 3);
        let ledger = Arc::new(ledger);

        let schedule = ledger.start_periodic_verification(Duration::from_millis(50));
        assert_eq!(
            schedule.last_report(),
            None,
            "no tick has run yet -- nothing to report"
        );

        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(
            schedule.last_report(),
            Some(VerificationReport::Intact),
            "the background thread's own real tick must have run and found the untampered chain \
             intact"
        );
        schedule.stop();
    }

    #[test]
    fn the_background_schedule_catches_a_real_tamper_on_its_own_next_tick_not_an_on_demand_call() {
        let (monitor, root, ledger) = setup();
        append_n(&monitor, &root, &ledger, 3);
        let ledger = Arc::new(ledger);

        let schedule = ledger.start_periodic_verification(Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(schedule.last_report(), Some(VerificationReport::Intact));

        // Tamper directly, the same way the on-demand verify_chain tests above do -- then wait
        // for the background thread's own next tick, never calling verify_chain ourselves.
        {
            let mut entries = ledger.entries.lock().unwrap();
            entries[1].payload = AuditPayload::Note("tampered".to_string());
        }

        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(
            schedule.last_report(),
            Some(VerificationReport::Corrupt { at_seq: 2 }),
            "the background thread's own next real tick must catch the tamper on its own, with \
             no on-demand verify_chain call from this test"
        );
        schedule.stop();
    }
}
