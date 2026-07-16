use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_recovery::RecoveryPointId;

use crate::types::{
    ActionId, Alternative, CalibrationScore, ConfidenceScore, ControlState, EvidenceRef,
    ExplainabilityError, ExplanationId, ExplanationRecord, ReasoningStep,
};

/// docs/18 §5's explain-then-commit: a record is assembled *during*
/// reasoning (append-as-you-go via [`Self::begin`]/[`Self::append_step`]),
/// never reconstructed after the fact — effect commit (docs/18 §5's
/// `control_state = EXECUTING` then `COMPLETED`) is gated on record
/// commit, per this crate's doc comment on the "no orphaned unexplained
/// effect" invariant.
pub struct ExplanationStore {
    records: Mutex<HashMap<ExplanationId, ExplanationRecord>>,
    next_id: AtomicU64,
    /// docs/998-roadmap.md's own named "workspace-wide, shared Explanation Record store" gap:
    /// `hyperion-coordination`/`hyperion-federation`/`hyperion-api-gateway` each used to mint
    /// their own private `action_id`s from an owner-local counter. Sharing one `ExplanationStore`
    /// across owners without also sharing *this* would let two different owners' `action_id`s
    /// collide (both starting at 1) — [`Self::get_by_action`]/`resolve_why`'s own
    /// first-match-by-`action_id` lookup would then silently resolve to the wrong owner's record.
    /// Minting every real `action_id` from this one counter, regardless of which owner asked,
    /// makes that collision structurally impossible rather than merely unlikely.
    next_action_id: AtomicU64,
}

impl Default for ExplanationStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ExplanationStore {
    pub fn new() -> Self {
        ExplanationStore {
            records: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            next_action_id: AtomicU64::new(1),
        }
    }

    /// A real, globally-unique-per-store `ActionId` — see this struct's own field doc comment on
    /// why every real owner of a shared store must mint through here rather than an owner-local
    /// counter of its own.
    pub fn next_action_id(&self) -> ActionId {
        self.next_action_id.fetch_add(1, Ordering::Relaxed)
    }

    fn require(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        rights: RightsMask,
    ) -> Result<(), ExplainabilityError> {
        monitor
            .check_rights_ok_result(token, rights)
            .map_err(|_| ExplainabilityError::Unauthorized)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        action_id: ActionId,
        triggering_intent_id: u64,
        agent_id: u64,
        capability_ref: &str,
        trust_boundary_span: Vec<u64>,
        now: u64,
    ) -> Result<ExplanationId, ExplainabilityError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.records.lock().unwrap().insert(
            id,
            ExplanationRecord {
                id,
                action_id,
                triggering_intent_id,
                agent_id,
                capability_ref: capability_ref.to_string(),
                created_at: now,
                reasoning_chain: Vec::new(),
                evidence: Vec::new(),
                confidence: None,
                alternatives: Vec::new(),
                undo_ref: None,
                trust_boundary_span,
                privacy_class: None,
                parent_records: Vec::new(),
                child_records: Vec::new(),
                control_state: ControlState::Proposed,
            },
        );
        Ok(id)
    }

    fn with_record_mut<T>(
        &self,
        id: ExplanationId,
        f: impl FnOnce(&mut ExplanationRecord) -> T,
    ) -> Result<T, ExplainabilityError> {
        let mut records = self.records.lock().unwrap();
        let record = records
            .get_mut(&id)
            .ok_or(ExplainabilityError::NoSuchRecord)?;
        Ok(f(record))
    }

    pub fn append_step(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: ExplanationId,
        step: ReasoningStep,
        evidence: Vec<EvidenceRef>,
    ) -> Result<(), ExplainabilityError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        self.with_record_mut(id, |record| {
            record.reasoning_chain.push(step);
            record.evidence.extend(evidence);
        })
    }

    pub fn set_confidence(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: ExplanationId,
        confidence: ConfidenceScore,
        alternatives: Vec<Alternative>,
    ) -> Result<(), ExplainabilityError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        self.with_record_mut(id, |record| {
            record.confidence = Some(confidence);
            record.alternatives = alternatives;
        })
    }

    /// docs/18 §7: `record.undo_ref = risk.recovery_point_ref` — "this
    /// function trusts and records that decision rather than re-deriving
    /// it." A caller passes through whatever
    /// `hyperion_security::RiskAssessment::recovery_point_ref` produced.
    pub fn attach_undo_ref(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: ExplanationId,
        recovery_point: RecoveryPointId,
    ) -> Result<(), ExplainabilityError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        self.with_record_mut(id, |record| record.undo_ref = Some(recovery_point))
    }

    /// docs/18 §5's multi-agent merge: each contributing Agent writes its
    /// own record, linked via `parent_records`/`child_records`.
    pub fn link_parent(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        child_id: ExplanationId,
        parent_id: ExplanationId,
    ) -> Result<(), ExplainabilityError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        {
            let mut records = self.records.lock().unwrap();
            if !records.contains_key(&parent_id) {
                return Err(ExplainabilityError::NoSuchRecord);
            }
            let child = records
                .get_mut(&child_id)
                .ok_or(ExplainabilityError::NoSuchRecord)?;
            child.parent_records.push(parent_id);
        }
        let mut records = self.records.lock().unwrap();
        records
            .get_mut(&parent_id)
            .unwrap()
            .child_records
            .push(child_id);
        Ok(())
    }

    pub fn transition(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: ExplanationId,
        state: ControlState,
    ) -> Result<(), ExplainabilityError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        self.with_record_mut(id, |record| record.control_state = state)
    }

    pub fn get(&self, id: ExplanationId) -> Option<ExplanationRecord> {
        self.records.lock().unwrap().get(&id).cloned()
    }

    fn get_by_action(&self, action_id: ActionId) -> Option<ExplanationRecord> {
        self.records
            .lock()
            .unwrap()
            .values()
            .find(|r| r.action_id == action_id)
            .cloned()
    }

    /// docs/18 §6's `explain.trace(intent_id) -> ExplanationGraph` —
    /// every action recorded under one Intent.
    pub fn trace_intent(&self, intent_id: u64) -> Vec<ExplanationRecord> {
        self.records
            .lock()
            .unwrap()
            .values()
            .filter(|r| r.triggering_intent_id == intent_id)
            .cloned()
            .collect()
    }

    /// docs/18 §9's completeness invariant: "no effect survives without a
    /// matching completed record." Surfaces every record whose
    /// `control_state` never reached a terminal state — the fault-
    /// injection assertion docs/18 §13 describes, exposed here as a
    /// direct query rather than a background checker this crate doesn't
    /// run.
    pub fn incomplete(&self) -> Vec<ExplanationRecord> {
        self.records
            .lock()
            .unwrap()
            .values()
            .filter(|r| {
                !matches!(
                    r.control_state,
                    ControlState::Completed | ControlState::RolledBack
                )
            })
            .cloned()
            .collect()
    }

    /// docs/18 §10/§13's "rolling Brier score per Agent/Capability" — see [`crate::calibration`]'s
    /// own doc comment for the real scoring algorithm and alert threshold. Computed over every
    /// real record this store currently holds for `(agent_id, capability_ref)`; `None` if there
    /// are none yet with both a real `confidence` and a real terminal outcome to score.
    pub fn calibration_score(
        &self,
        agent_id: u64,
        capability_ref: &str,
    ) -> Option<CalibrationScore> {
        let records: Vec<ExplanationRecord> =
            self.records.lock().unwrap().values().cloned().collect();
        crate::calibration::calibration_score(&records, agent_id, capability_ref)
    }
}

pub(crate) fn resolve_by_action(
    store: &ExplanationStore,
    action_id: ActionId,
) -> Option<ExplanationRecord> {
    store.get_by_action(action_id)
}
