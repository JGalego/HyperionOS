use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::types::{
    ActionId, Alternative, CalibrationScore, ConfidenceScore, ControlState, EvidenceRef,
    ExplainabilityError, ExplanationId, ExplanationLookup, ExplanationRecord, ReasoningStep,
    RecoveryPointId,
};
use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_events::{
    BackpressurePolicy, DeliveryClass, EventBus, EventPayload, SubjectId, SubscriptionId, Topic,
    TopicKind, TopicPattern,
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
    /// docs/18 §9's own named "`best_effort_reconstruction` via the Event System (31)" gap:
    /// real, optional (the same `Option<...>` shape this workspace uses for every other optional
    /// backend) once [`Self::with_events`] wires it. Holds the id of the one long-lived, Durable,
    /// kind-wide subscription every record's `begin`/`append_step`/`transition` call publishes an
    /// event onto — see [`Self::with_events`]'s own doc comment for why one shared subscription,
    /// not one per record.
    events: Option<(Arc<EventBus>, SubscriptionId)>,
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
            events: None,
        }
    }

    /// Opts this store into docs/31-event-system.md's real broadcast bus, closing this crate's
    /// own previously-named "no Event System crate exists yet anywhere in this workspace to
    /// replay from" gap. Opens one long-lived, `AtLeastOnce`/`Durable`,
    /// `TopicPattern::KindScoped(TopicKind::Custom)` subscription immediately — a single shared
    /// log every record's `begin`/`append_step`/`transition` call publishes an event onto, not
    /// one subscription per record, since this store cannot know in advance which id will ever
    /// need reconstructing. `admin_token` must carry `GRANT` (`hyperion_events::EventBus`'s own
    /// `KindScoped` authorization rule) — the same elevated, cross-subject visibility a docs/34
    /// audit sink already needs for the identical reason. Every event is still published under
    /// its own record's real Trust-Boundary owner (the token that called `begin`); this admin
    /// subscription only grants this *store* the right to receive them all so it has something to
    /// replay from later — [`Self::get_or_reconstruct`] re-applies the caller's own
    /// Trust-Boundary check before ever returning a reconstructed record, exactly like
    /// [`Self::get`] already does for an authoritative one.
    pub fn with_events(
        mut self,
        monitor: &CapabilityMonitor,
        admin_token: &CapabilityToken,
        events: Arc<EventBus>,
    ) -> Result<Self, ExplainabilityError> {
        let sub = events
            .subscribe(
                monitor,
                admin_token,
                admin_token.origin(),
                TopicPattern::KindScoped(TopicKind::Custom),
                DeliveryClass::AtLeastOnce,
                BackpressurePolicy::Durable,
            )
            .map_err(|_| ExplainabilityError::Unauthorized)?;
        self.events = Some((events, sub.id()));
        Ok(self)
    }

    fn publish_record_event(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: ExplanationId,
        payload: serde_json::Value,
    ) {
        let Some((bus, _)) = &self.events else {
            return;
        };
        let topic = Topic::new(
            TopicKind::Custom,
            SubjectId::Object(id),
            "explainability.record_event.v1",
        );
        if let Err(e) = bus.publish(
            monitor,
            token,
            token.origin(),
            topic,
            EventPayload::Inline(payload),
            Vec::new(),
        ) {
            eprintln!("hyperion-explainability: failed to publish record event: {e}");
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

    /// docs/18 §8's own "access to an `explain.query` result is gated by the same capability
    /// grant that gated the underlying data" needs a real per-record Trust Boundary to check a
    /// reader's token against — [`ExplanationRecord::trust_boundary_span`] existed for this, but
    /// every real caller passed it as a dead, hardcoded `vec![]`, since nothing ever read it
    /// back (see this crate's own doc comment). `trust_boundary_span` is now always seeded with
    /// this real, live `token.origin().0` (2026-07-16) — a caller's own explicit span (docs/18
    /// §5's multi-agent merge, where more than one boundary genuinely contributed) is preserved
    /// and simply extended if it doesn't already include the boundary actually opening this
    /// record.
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
        let caller_boundary = token.origin().0;
        let mut trust_boundary_span = trust_boundary_span;
        if !trust_boundary_span.contains(&caller_boundary) {
            trust_boundary_span.push(caller_boundary);
        }
        self.publish_record_event(
            monitor,
            token,
            id,
            serde_json::json!({
                "kind": "begin",
                "action_id": action_id,
                "triggering_intent_id": triggering_intent_id,
                "agent_id": agent_id,
                "capability_ref": capability_ref,
                "created_at": now,
                "trust_boundary_span": trust_boundary_span,
            }),
        );
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
        let event_payload = serde_json::json!({
            "kind": "step",
            "step_index": step.step_index,
            "description": step.description,
            "capability_ref": step.capability_ref,
        });
        self.with_record_mut(id, |record| {
            record.reasoning_chain.push(step);
            record.evidence.extend(evidence);
        })?;
        self.publish_record_event(monitor, token, id, event_payload);
        Ok(())
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
        self.with_record_mut(id, |record| record.control_state = state)?;
        self.publish_record_event(
            monitor,
            token,
            id,
            serde_json::json!({
                "kind": "transition",
                "state": format!("{state:?}"),
            }),
        );
        Ok(())
    }

    /// docs/18 §8's own "access to an `explain.query` result is gated by the same capability
    /// grant that gated the underlying data — a user... cannot use the explanation channel as a
    /// side door to read data they were never granted access to," real for the first time
    /// (2026-07-16): every read here, not just every write, now checks `RightsMask::READ` and
    /// filters by [`ExplanationRecord::trust_boundary_span`] — a record whose span doesn't
    /// include the caller's own `token.origin()` is `None`/omitted, never an error that would
    /// reveal it exists under a boundary the caller can't see.
    pub fn get(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: ExplanationId,
    ) -> Result<Option<ExplanationRecord>, ExplainabilityError> {
        self.require(monitor, token, RightsMask::READ)?;
        let caller_boundary = token.origin().0;
        Ok(self
            .records
            .lock()
            .unwrap()
            .get(&id)
            .filter(|r| r.trust_boundary_span.contains(&caller_boundary))
            .cloned())
    }

    /// docs/18 §9's degrade path: "`explain.query` degrades to `best_effort_reconstruction`,
    /// replaying [31 — Event System] logs to approximate the record and flagging the result as
    /// reconstructed, never presenting a best-effort guess as an authoritative record." Tries
    /// [`Self::get`] first; only replays the durable log (via [`Self::with_events`]'s own admin
    /// subscription) when the real record is genuinely absent — e.g. the in-memory store never
    /// had it (a real crash before this in-process `HashMap` was populated) or was never wired.
    /// Re-applies the exact same Trust-Boundary check `Self::get` does (via the `begin` event's
    /// own published `trust_boundary_span`) before ever returning a reconstructed record — the
    /// admin subscription's own broad `GRANT`-based visibility must never become a side door
    /// around a caller's real access grant.
    pub fn get_or_reconstruct(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        id: ExplanationId,
    ) -> Result<Option<ExplanationLookup>, ExplainabilityError> {
        if let Some(record) = self.get(monitor, token, id)? {
            return Ok(Some(ExplanationLookup::Authoritative(record)));
        }
        // `get`'s own `RightsMask::READ` check already ran above; a caller without it never
        // reaches this reconstruction path either.
        let Some((bus, sub_id)) = &self.events else {
            return Ok(None);
        };
        let mut relevant: Vec<_> = bus
            .replay_from(*sub_id, 0)
            .map_err(|_| ExplainabilityError::NoSuchRecord)?
            .into_iter()
            .filter(|e| e.topic.subject.raw() == id)
            .collect();
        relevant.sort_by_key(|e| e.seq);
        if relevant.is_empty() {
            return Ok(None);
        }

        let caller_boundary = token.origin().0;
        let mut action_id: ActionId = 0;
        let mut triggering_intent_id = 0;
        let mut agent_id = 0;
        let mut capability_ref = String::new();
        let mut created_at = 0;
        let mut trust_boundary_span: Vec<u64> = Vec::new();
        let mut reasoning_chain = Vec::new();
        let mut control_state = ControlState::Proposed;

        for event in &relevant {
            let EventPayload::Inline(payload) = &event.payload else {
                continue;
            };
            match payload["kind"].as_str() {
                Some("begin") => {
                    action_id = payload["action_id"].as_u64().unwrap_or(0);
                    triggering_intent_id = payload["triggering_intent_id"].as_u64().unwrap_or(0);
                    agent_id = payload["agent_id"].as_u64().unwrap_or(0);
                    capability_ref = payload["capability_ref"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    created_at = payload["created_at"].as_u64().unwrap_or(0);
                    trust_boundary_span = payload["trust_boundary_span"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
                        .unwrap_or_default();
                }
                Some("step") => {
                    reasoning_chain.push(ReasoningStep {
                        step_index: payload["step_index"].as_u64().unwrap_or(0) as u32,
                        description: payload["description"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        capability_ref: payload["capability_ref"].as_str().map(String::from),
                        inputs_ref: Vec::new(),
                        output_ref: None,
                    });
                }
                Some("transition") => {
                    control_state = match payload["state"].as_str() {
                        Some("Executing") => ControlState::Executing,
                        Some("Completed") => ControlState::Completed,
                        Some("Interrupted") => ControlState::Interrupted,
                        Some("Modified") => ControlState::Modified,
                        Some("RolledBack") => ControlState::RolledBack,
                        _ => ControlState::Proposed,
                    };
                }
                _ => {}
            }
        }

        if !trust_boundary_span.contains(&caller_boundary) {
            // Same "never reveal existence of what you can't see" discipline as `Self::get`.
            return Ok(None);
        }

        Ok(Some(ExplanationLookup::Reconstructed(ExplanationRecord {
            id,
            action_id,
            triggering_intent_id,
            agent_id,
            capability_ref,
            created_at,
            reasoning_chain,
            evidence: Vec::new(),
            confidence: None,
            alternatives: Vec::new(),
            undo_ref: None,
            trust_boundary_span,
            privacy_class: None,
            parent_records: Vec::new(),
            child_records: Vec::new(),
            control_state,
        })))
    }

    /// As [`Self::get_or_reconstruct`], but resolved by `action_id` — `resolve_why`'s own real
    /// entry point (docs/18 §5/§6: callers know the effect's `action_id`, never an internal
    /// record id). The real record is missing *and* unindexable by `action_id` once it's gone
    /// from this store's in-memory map, so reconstruction here first finds which record's own
    /// `begin` event claimed this `action_id` (a full, unfiltered scan of the durable log — this
    /// store keeps no separate `action_id` index over reconstructable history, only over live
    /// records), then reconstructs that one record exactly as [`Self::get_or_reconstruct`] would.
    pub fn get_or_reconstruct_by_action(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        action_id: ActionId,
    ) -> Result<Option<ExplanationLookup>, ExplainabilityError> {
        if let Some(record) = self.get_by_action(monitor, token, action_id)? {
            return Ok(Some(ExplanationLookup::Authoritative(record)));
        }
        self.require(monitor, token, RightsMask::READ)?;
        let Some((bus, sub_id)) = &self.events else {
            return Ok(None);
        };
        let all = bus
            .replay_from(*sub_id, 0)
            .map_err(|_| ExplainabilityError::NoSuchRecord)?;
        let target_id = all.iter().find_map(|e| {
            let EventPayload::Inline(payload) = &e.payload else {
                return None;
            };
            if payload["kind"] == "begin" && payload["action_id"].as_u64() == Some(action_id) {
                Some(e.topic.subject.raw())
            } else {
                None
            }
        });
        match target_id {
            Some(id) => self.get_or_reconstruct(monitor, token, id),
            None => Ok(None),
        }
    }

    fn get_by_action(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        action_id: ActionId,
    ) -> Result<Option<ExplanationRecord>, ExplainabilityError> {
        self.require(monitor, token, RightsMask::READ)?;
        let caller_boundary = token.origin().0;
        Ok(self
            .records
            .lock()
            .unwrap()
            .values()
            .find(|r| r.action_id == action_id && r.trust_boundary_span.contains(&caller_boundary))
            .cloned())
    }

    /// docs/18 §6's `explain.trace(intent_id) -> ExplanationGraph` —
    /// every action recorded under one Intent. Trust-Boundary-filtered (2026-07-16) the same way
    /// [`Self::get`] now is.
    pub fn trace_intent(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        intent_id: u64,
    ) -> Result<Vec<ExplanationRecord>, ExplainabilityError> {
        self.require(monitor, token, RightsMask::READ)?;
        let caller_boundary = token.origin().0;
        Ok(self
            .records
            .lock()
            .unwrap()
            .values()
            .filter(|r| {
                r.triggering_intent_id == intent_id
                    && r.trust_boundary_span.contains(&caller_boundary)
            })
            .cloned()
            .collect())
    }

    /// docs/18 §9's completeness invariant: "no effect survives without a
    /// matching completed record." Surfaces every record whose
    /// `control_state` never reached a terminal state — the fault-
    /// injection assertion docs/18 §13 describes, exposed here as a
    /// direct query rather than a background checker this crate doesn't
    /// run. Trust-Boundary-filtered (2026-07-16) the same way [`Self::get`] now is.
    pub fn incomplete(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
    ) -> Result<Vec<ExplanationRecord>, ExplainabilityError> {
        self.require(monitor, token, RightsMask::READ)?;
        let caller_boundary = token.origin().0;
        Ok(self
            .records
            .lock()
            .unwrap()
            .values()
            .filter(|r| {
                r.trust_boundary_span.contains(&caller_boundary)
                    && !matches!(
                        r.control_state,
                        ControlState::Completed | ControlState::RolledBack
                    )
            })
            .cloned()
            .collect())
    }

    /// docs/18 §10/§13's "rolling Brier score per Agent/Capability" — see [`crate::calibration`]'s
    /// own doc comment for the real scoring algorithm and alert threshold. Computed over every
    /// real record this store currently holds for `(agent_id, capability_ref)`; `None` if there
    /// are none yet with both a real `confidence` and a real terminal outcome to score.
    /// Trust-Boundary-filtered (2026-07-16) the same way [`Self::get`] now is: only the caller's
    /// own visible records ever feed the score, per docs/18 §8's same "same capability grant"
    /// gate.
    pub fn calibration_score(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        agent_id: u64,
        capability_ref: &str,
    ) -> Result<Option<CalibrationScore>, ExplainabilityError> {
        self.require(monitor, token, RightsMask::READ)?;
        let caller_boundary = token.origin().0;
        let records: Vec<ExplanationRecord> = self
            .records
            .lock()
            .unwrap()
            .values()
            .filter(|r| r.trust_boundary_span.contains(&caller_boundary))
            .cloned()
            .collect();
        Ok(crate::calibration::calibration_score(
            &records,
            agent_id,
            capability_ref,
        ))
    }
}
