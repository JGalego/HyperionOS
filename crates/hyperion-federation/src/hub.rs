use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hyperion_agent_runtime::{AgentManifest, AgentRuntime, InvokeOutcome};
use hyperion_ai_runtime::{
    sign, LocalAiRuntime, MockBackend, ModelClass, ModelDescriptor, Precision, QuantizedVariant,
};
use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_crypto::Keystore;
use hyperion_explainability::{
    ControlState, ExplanationId, ExplanationRecord, ExplanationStore, ReasoningStep,
};
use hyperion_observability::{TelemetryCollector, TraceId};
use hyperion_scheduler::{ResourceDimension, ResourceVector};

use crate::types::{
    AnchorLease, FederationTrustTier, MigrationOutcome, MigrationReceipt, OffloadDescriptor,
    PrivacyTier, VirtualResourceLedger,
};

#[derive(Debug, thiserror::Error)]
pub enum FederationError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("no such device in this federation")]
    NoSuchDevice,
    #[error("no such agent instance")]
    NoSuchAgent,
    #[error("no candidate device could satisfy this offload")]
    NoFeasiblePlacement,
    #[error("lease held by a more (or equally) authoritative device")]
    LeaseConflict,
    #[error("no such anchor lease")]
    NoSuchLease,
    #[error("only the current anchor device may initiate this operation")]
    NotAuthoritative,
    #[error("agent runtime error: {0}")]
    Agent(#[from] hyperion_agent_runtime::AgentError),
    #[error("explainability error: {0}")]
    Explainability(#[from] hyperion_explainability::ExplainabilityError),
}

#[derive(Debug, Clone, Copy)]
struct AgentRef {
    device_id: u64,
    local_instance: u64,
}

/// A real, running background thread that automatically renews an [`AnchorLease`] on a fixed
/// real wall-clock interval — see [`FederationHub::start_lease_heartbeat`]. Stopped by dropping
/// this handle (or calling [`Self::stop`] explicitly) — the real background thread is joined, not
/// merely detached, so a caller can be sure it has genuinely stopped renewing before proceeding
/// (e.g. before releasing the lease it was renewing, to avoid a benign but confusing race).
pub struct LeaseHeartbeat {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl LeaseHeartbeat {
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
}

impl Drop for LeaseHeartbeat {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

/// docs/21 — Distributed Execution. See this crate's doc comment for what's
/// deferred.
pub struct FederationHub {
    devices: Mutex<HashMap<u64, Arc<AgentRuntime>>>,
    trust_tiers: Mutex<HashMap<u64, FederationTrustTier>>,
    ledgers: Mutex<HashMap<u64, VirtualResourceLedger>>,
    leases: Mutex<HashMap<u64, AnchorLease>>,
    agents: Mutex<HashMap<u64, AgentRef>>,
    next_agent_id: AtomicU64,
    next_migration_id: AtomicU64,
    /// docs/18's Explanation Record store for this hub's own
    /// `dispatch_offload`/`invoke_agent` dispatches — see those methods
    /// and [`Self::explanation`]/[`Self::trace_intent`]. `Arc`-shared so
    /// [`Self::new_with_shared_explanations`] can hand this hub the same
    /// real store `hyperion-coordination`/`hyperion-api-gateway` use —
    /// docs/998-roadmap.md's own named "workspace-wide, shared Explanation
    /// Record store" gap, closed for a caller that wants it.
    explanations: Arc<ExplanationStore>,
    /// One real `hyperion-observability` `TelemetryCollector` per device,
    /// mirroring `devices` — [`Self::migrate`] is the real production call
    /// site for `TelemetryCollector::merge_remote_trace` docs/21's own
    /// distributed trace merging names: it pulls whatever the source
    /// device recorded under a migrating agent's `trace_id` into the
    /// target device's collector, so a caller querying the target after
    /// migration sees the whole cross-device trace, not just what ran
    /// there after the hop.
    telemetry: Mutex<HashMap<u64, Arc<TelemetryCollector>>>,
    /// This hub's own real Ed25519 identity, shared by every device it federates today (see
    /// [`Self::seal`]/[`Self::open`]'s own doc comment on why that's an honest scope boundary,
    /// not an oversight) -- [`Self::new`] gives every hub a real, ephemeral
    /// (`Keystore::ephemeral`) one so existing callers need no changes; [`Self::new_with_keystore`]
    /// is for a caller with a real, persisted identity to use instead.
    keystore: Keystore,
}

impl Default for FederationHub {
    fn default() -> Self {
        Self::new()
    }
}

impl FederationHub {
    pub fn new() -> Self {
        Self::new_with_keystore(Keystore::ephemeral())
    }

    /// As [`Self::new`], with a real, caller-supplied identity (e.g. one persisted via
    /// [`Keystore::open_or_create`]) instead of a fresh, process-lifetime-only one.
    pub fn new_with_keystore(keystore: Keystore) -> Self {
        Self::new_with_shared_explanations(keystore, Arc::new(ExplanationStore::new()))
    }

    /// As [`Self::new_with_keystore`], but with a real, caller-supplied [`ExplanationStore`] this
    /// hub shares with other real owners (e.g. a `hyperion_coordination::CoordinationSession` or
    /// `hyperion_api_gateway::ApiGateway` in the same process) instead of its own private one —
    /// a single, real, workspace-wide trace instead of several independent ones. Every real
    /// `action_id` this hub mints for it comes from the store's own
    /// [`hyperion_explainability::ExplanationStore::next_action_id`], not an owner-local counter
    /// of its own — sharing a store without also sharing that counter would let two different
    /// owners' `action_id`s collide.
    pub fn new_with_shared_explanations(
        keystore: Keystore,
        explanations: Arc<ExplanationStore>,
    ) -> Self {
        FederationHub {
            devices: Mutex::new(HashMap::new()),
            trust_tiers: Mutex::new(HashMap::new()),
            ledgers: Mutex::new(HashMap::new()),
            leases: Mutex::new(HashMap::new()),
            agents: Mutex::new(HashMap::new()),
            next_agent_id: AtomicU64::new(1),
            next_migration_id: AtomicU64::new(1),
            explanations,
            telemetry: Mutex::new(HashMap::new()),
            keystore,
        }
    }

    /// docs/998-roadmap.md's own named "`SyncEnvelope`-wrapped encrypted payloads" gap, closed
    /// for real: really encrypts (ChaCha20-Poly1305) and really signs (Ed25519) `plaintext` via
    /// `hyperion_crypto::sync_envelope`, using this hub's own real identity. `sender_device_id`
    /// is a real, signature-covered provenance label, not a secret -- it's the caller's job to
    /// pass the id of whichever device is actually producing `plaintext`.
    ///
    /// **Honest scope**: every device this hub federates shares this *one* symmetric key today
    /// (one process, one hub, one identity) -- for a real, independently-keyed *peer* hub, see
    /// [`Self::seal_for_peer`]/[`Self::open_from_peer`] below instead.
    pub fn seal(&self, sender_device_id: u64, plaintext: &[u8]) -> hyperion_crypto::SyncEnvelope {
        hyperion_crypto::sync_envelope::seal(&self.keystore, sender_device_id, plaintext)
    }

    /// The real inverse of [`Self::seal`] -- a tampered or wrongly-keyed envelope is a real,
    /// honest [`hyperion_crypto::SyncEnvelopeError`], never a silent or partial decrypt.
    pub fn open(
        &self,
        envelope: &hyperion_crypto::SyncEnvelope,
    ) -> Result<Vec<u8>, hyperion_crypto::SyncEnvelopeError> {
        hyperion_crypto::sync_envelope::open(&self.keystore, envelope)
    }

    /// This hub's own real, public Ed25519 verifying key -- what a peer needs to authenticate a
    /// [`Self::seal_for_peer`]-sealed envelope's real signature via [`Self::open_from_peer`].
    pub fn verifying_key(&self) -> hyperion_crypto::VerifyingKey {
        self.keystore.verifying_key()
    }

    /// This hub's own real X25519 public key -- what a genuinely independent peer hub needs to
    /// derive the same real shared secret via [`Self::establish_shared_secret`]. Closes the gap
    /// [`Self::seal`]/[`Self::open`]'s own doc comment names: "not yet a real key-exchange between
    /// genuinely separate, independently-keyed devices."
    pub fn x25519_public(&self) -> hyperion_crypto::X25519PublicKey {
        self.keystore.x25519_public()
    }

    /// A real X25519 Diffie-Hellman shared secret between this hub and a peer hub identified only
    /// by `their_x25519_public` -- the caller passes it to [`Self::seal_for_peer`]/
    /// [`Self::open_from_peer`], never persisted or cached here (recomputing it is cheap, and this
    /// hub does not track which peers it's ever exchanged keys with).
    pub fn establish_shared_secret(
        &self,
        their_x25519_public: &hyperion_crypto::X25519PublicKey,
    ) -> [u8; 32] {
        hyperion_crypto::diffie_hellman(&self.keystore, their_x25519_public)
    }

    /// Really encrypts `plaintext` for a genuinely independent peer hub, keyed by a real
    /// `shared_secret` from [`Self::establish_shared_secret`] (never this hub's own [`Self::seal`]
    /// key, which only ever matches another sealer sharing this exact hub's `Keystore`). Still
    /// really signs with this hub's own identity, so the receiving peer can verify genuine
    /// authorship via [`Self::open_from_peer`]-equivalent logic on its own side.
    pub fn seal_for_peer(
        &self,
        shared_secret: &[u8; 32],
        sender_device_id: u64,
        plaintext: &[u8],
    ) -> hyperion_crypto::SyncEnvelope {
        hyperion_crypto::sync_envelope::seal_for_peer(
            &self.keystore,
            shared_secret,
            sender_device_id,
            plaintext,
        )
    }

    /// The real inverse of [`Self::seal_for_peer`]: verifies against `sender_verifying_key` (the
    /// *peer's* real public signing key, not this hub's own -- get it from the peer out of band,
    /// e.g. alongside its `x25519_public()`), then really decrypts with the same real
    /// `shared_secret`. Either failing is a real, honest error, exactly as [`Self::open`] already
    /// guarantees for the single-hub case.
    pub fn open_from_peer(
        &self,
        sender_verifying_key: &hyperion_crypto::VerifyingKey,
        shared_secret: &[u8; 32],
        envelope: &hyperion_crypto::SyncEnvelope,
    ) -> Result<Vec<u8>, hyperion_crypto::SyncEnvelopeError> {
        hyperion_crypto::sync_envelope::open_from_peer(
            sender_verifying_key,
            shared_secret,
            envelope,
        )
    }

    /// The real `hyperion-observability` `TelemetryCollector`
    /// [`Self::join_device`] minted for `device_id` — a caller records
    /// real spans/logs against it exactly as it would for any other
    /// device-local telemetry source, and [`Self::migrate`] reads from it
    /// to reconstruct cross-device continuity.
    pub fn telemetry_for(&self, device_id: u64) -> Option<Arc<TelemetryCollector>> {
        self.telemetry.lock().unwrap().get(&device_id).cloned()
    }

    /// docs/18's "queryable Explanation Record" surface for this hub's
    /// own dispatches — see [`Self::dispatch_offload`]/[`Self::invoke_agent`].
    pub fn explanation(&self, id: ExplanationId) -> Option<ExplanationRecord> {
        self.explanations.get(id)
    }

    /// Every record this hub has opened under `intent_id` —
    /// [`Self::dispatch_offload`]/[`Self::invoke_agent`] both take a real,
    /// caller-supplied `triggering_intent_id` now, so this is a genuine
    /// correlation whenever the caller passes one from a real
    /// `hyperion_intent::IntentEngine::submit`, not a hardcoded sentinel.
    pub fn trace_intent(&self, intent_id: u64) -> Vec<ExplanationRecord> {
        self.explanations.trace_intent(intent_id)
    }

    fn require(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        rights: RightsMask,
    ) -> Result<(), FederationError> {
        monitor
            .check_rights_ok_result(token, rights)
            .map_err(|_| FederationError::Unauthorized)
    }

    fn device(&self, device_id: u64) -> Result<Arc<AgentRuntime>, FederationError> {
        self.devices
            .lock()
            .unwrap()
            .get(&device_id)
            .cloned()
            .ok_or(FederationError::NoSuchDevice)
    }

    /// docs/21 §Algorithms' "Federation join and trust": an ordinary
    /// capability grant, one distinct Trust Boundary — a real, separate
    /// `AgentRuntime` instance — per device, each with its own
    /// `MockBackend`-fronted `LocalAiRuntime`. A real, previously-dormant gap this crate's own
    /// doc comment used to note ("no capability this crate dispatches ever calls
    /// `assistant.respond` today") is no longer dormant: `hyperion-agent-runtime`'s own fix for
    /// the "launch my startup produces zero real content" gap made `web.search` (this crate's own
    /// baseline capability for every joined device) dispatch through a real `LocalAiRuntime::
    /// infer` call too, which -- like `assistant.respond` always has -- fails closed with no
    /// model registered. [`Self::register_simulated_model`] closes it the same way
    /// `hyperion-console`'s own `build_ai_runtime` always has: a small, real, signed
    /// `ModelDescriptor` registered before this device's runtime is ever handed out.
    pub fn join_device(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        device_id: u64,
        trust_tier: FederationTrustTier,
    ) -> Result<(), FederationError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));
        Self::register_simulated_model(&ai_runtime);
        self.devices
            .lock()
            .unwrap()
            .insert(device_id, Arc::new(AgentRuntime::new(ai_runtime)));
        self.trust_tiers
            .lock()
            .unwrap()
            .insert(device_id, trust_tier);
        self.telemetry
            .lock()
            .unwrap()
            .insert(device_id, Arc::new(TelemetryCollector::new()));
        Ok(())
    }

    /// Registers one small, real, signed `ModelDescriptor` on a freshly-created device's own
    /// `LocalAiRuntime` -- see [`Self::join_device`]'s own doc comment for why this exists now.
    /// The signing key is a genuine, real Ed25519 key (`hyperion_ai_runtime::sign` hard-requires
    /// one), generated fresh in its own uniquely-named temp directory (`tempfile::tempdir`, not a
    /// fixed path -- this runs on every `join_device` call, including from parallel tests in the
    /// same process, so it must never collide with another call's own key file) and dropped the
    /// moment this function returns: this hub has no real, lasting per-device identity to reuse,
    /// and doesn't need one just to prove a simulated device's own local inference is genuinely
    /// callable. Degrades silently (not a panic) if even a throwaway temp file can't be written --
    /// an extreme, environment-level failure this crate's own callers already have no path to
    /// react to inside `join_device`'s existing, infallible-past-this-point signature.
    fn register_simulated_model(ai_runtime: &LocalAiRuntime) {
        let Ok(dir) = tempfile::tempdir() else {
            return;
        };
        let Ok(keystore) = Keystore::open_or_create(&dir.path().join("device.key")) else {
            return;
        };
        let mut descriptor = ModelDescriptor {
            model_id: 1,
            class: ModelClass::Slm,
            variants: vec![QuantizedVariant {
                precision: Precision::Fp16,
                footprint_mb: 100,
                expected_tokens_per_sec: 10.0,
            }],
            signature: None,
        };
        descriptor.signature = Some(sign(&descriptor, &keystore));
        let _ = ai_runtime.register_model(descriptor, &keystore.verifying_key());
    }

    /// docs/21 §Security Considerations: "a compromised or stolen device's
    /// tokens fence off instantly." Removing a device tears down its
    /// ledger and Trust Boundary; any lease it held is left for the next
    /// `acquire_lease` conflict/expiry path to reclaim.
    pub fn remove_device(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        device_id: u64,
    ) -> Result<(), FederationError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        self.devices.lock().unwrap().remove(&device_id);
        self.trust_tiers.lock().unwrap().remove(&device_id);
        self.ledgers.lock().unwrap().remove(&device_id);
        self.telemetry.lock().unwrap().remove(&device_id);
        Ok(())
    }

    pub fn publish_ledger(
        &self,
        device_id: u64,
        available: ResourceVector,
        network_latency_ms: u32,
        now: u64,
        ttl_secs: u64,
    ) -> Result<(), FederationError> {
        let trust_tier = *self
            .trust_tiers
            .lock()
            .unwrap()
            .get(&device_id)
            .ok_or(FederationError::NoSuchDevice)?;
        self.ledgers.lock().unwrap().insert(
            device_id,
            VirtualResourceLedger {
                device_id,
                trust_tier,
                available,
                network_latency_ms,
                published_at: now,
                ttl_secs,
            },
        );
        Ok(())
    }

    fn fits(request: &ResourceVector, available: &ResourceVector) -> bool {
        ResourceDimension::ALL
            .iter()
            .all(|&d| request.get(d) <= available.get(d))
    }

    fn best_candidate(
        &self,
        descriptor: &OffloadDescriptor,
        excluded: &[u64],
        now: u64,
    ) -> Option<VirtualResourceLedger> {
        self.ledgers
            .lock()
            .unwrap()
            .values()
            .filter(|l| !excluded.contains(&l.device_id))
            .filter(|l| l.is_live(now))
            .filter(|l| {
                descriptor.privacy_tier == PrivacyTier::ConsentedCloud || !l.trust_tier.is_cloud()
            })
            .filter(|l| Self::fits(&descriptor.request, &l.available))
            .filter(|l| {
                descriptor
                    .deadline_ms
                    .is_none_or(|d| l.network_latency_ms <= d)
            })
            .min_by_key(|l| l.network_latency_ms)
            .copied()
    }

    /// docs/21 §Algorithms' "Task offload execution" + §Pseudocode
    /// `dispatch_offload`: the privacy gate excludes candidates before any
    /// scoring runs (never merely deprioritizes), and a candidate that
    /// fails on arrival is invalidated with an automatic retry against the
    /// next one, matching the doc's own retry loop. `triggering_intent_id`
    /// is a caller-supplied real `hyperion-intent` Intent `NodeId.0` — this
    /// crate does not itself depend on `hyperion-intent` (it has no need
    /// to read Intent Graph structure, only to attribute this dispatch's
    /// Explanation Record to whichever real Intent triggered it), so a
    /// caller that never calls `IntentEngine::submit` at all may still
    /// pass any sentinel `u64` it likes.
    #[allow(clippy::too_many_arguments)]
    pub fn dispatch_offload(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        descriptor: &OffloadDescriptor,
        capability_ref: &str,
        args: serde_json::Value,
        triggering_intent_id: u64,
        now: u64,
    ) -> Result<serde_json::Value, FederationError> {
        self.require(monitor, token, RightsMask::EXEC)?;

        let mut excluded = Vec::new();
        loop {
            let candidate = self
                .best_candidate(descriptor, &excluded, now)
                .ok_or(FederationError::NoFeasiblePlacement)?;
            let runtime = self.device(candidate.device_id)?;

            let manifest = AgentManifest {
                specialization: "offload".to_string(),
                baseline_capabilities: vec![capability_ref.to_string()],
                requestable_capabilities: Vec::new(),
                trust_tier: hyperion_agent_runtime::TrustTier::System,
            };
            let instance = runtime.spawn(monitor, token, manifest, None)?;

            let action_id = self.explanations.next_action_id();
            let explanation_id = self.explanations.begin(
                monitor,
                token,
                action_id,
                triggering_intent_id,
                instance,
                capability_ref,
                vec![],
                now,
            )?;
            self.explanations.append_step(
                monitor,
                token,
                explanation_id,
                ReasoningStep {
                    step_index: 0,
                    description: format!(
                        "offloaded to device {} (latency {}ms)",
                        candidate.device_id, candidate.network_latency_ms
                    ),
                    capability_ref: Some(capability_ref.to_string()),
                    inputs_ref: Vec::new(),
                    output_ref: None,
                },
                Vec::new(),
            )?;
            self.explanations.transition(
                monitor,
                token,
                explanation_id,
                ControlState::Executing,
            )?;

            let outcome = runtime.invoke(monitor, token, instance, capability_ref, args.clone())?;
            runtime.terminate(monitor, token, instance, "offload_complete")?;

            match outcome {
                InvokeOutcome::Result(value) => {
                    self.explanations.transition(
                        monitor,
                        token,
                        explanation_id,
                        ControlState::Completed,
                    )?;
                    return Ok(value);
                }
                _ => {
                    self.explanations.transition(
                        monitor,
                        token,
                        explanation_id,
                        ControlState::RolledBack,
                    )?;
                    excluded.push(candidate.device_id);
                    continue;
                }
            }
        }
    }

    /// docs/21 §Algorithms' "Anchor lease" + §Recovery Mechanisms' split-
    /// brain tie-break: higher `FederationTrustTier`, then lower
    /// `device_id`, wins a conflicting claim; the loser's request is
    /// rejected rather than silently overwriting the winner.
    pub fn acquire_lease(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        agent_instance: u64,
        device_id: u64,
        now: u64,
        ttl_secs: u64,
    ) -> Result<AnchorLease, FederationError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let requester_tier = *self
            .trust_tiers
            .lock()
            .unwrap()
            .get(&device_id)
            .ok_or(FederationError::NoSuchDevice)?;

        let mut leases = self.leases.lock().unwrap();
        let next_generation = if let Some(existing) = leases.get(&agent_instance) {
            if existing.holder_device == device_id {
                // The current holder refreshing its own claim — no
                // challenge, no generation bump (that's `renew_lease`'s
                // job too, but callers may also route through here).
                existing.generation
            } else if existing.is_live(now) {
                let holder_tier = *self
                    .trust_tiers
                    .lock()
                    .unwrap()
                    .get(&existing.holder_device)
                    .unwrap_or(&FederationTrustTier::CloudRented);
                let requester_key = (requester_tier.trust_rank(), std::cmp::Reverse(device_id));
                let holder_key = (
                    holder_tier.trust_rank(),
                    std::cmp::Reverse(existing.holder_device),
                );
                if requester_key <= holder_key {
                    return Err(FederationError::LeaseConflict);
                }
                existing.generation + 1
            } else {
                // Expired and held by a different device — freely
                // reclaimed, but the generation still advances so a
                // delayed message from the old holder is recognizably
                // stale.
                existing.generation + 1
            }
        } else {
            0
        };

        let lease = AnchorLease {
            agent_instance,
            holder_device: device_id,
            generation: next_generation,
            granted_at: now,
            ttl_secs,
        };
        leases.insert(agent_instance, lease);
        Ok(lease)
    }

    pub fn renew_lease(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        agent_instance: u64,
        device_id: u64,
        now: u64,
    ) -> Result<AnchorLease, FederationError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let mut leases = self.leases.lock().unwrap();
        let lease = leases
            .get_mut(&agent_instance)
            .ok_or(FederationError::NoSuchLease)?;
        if lease.holder_device != device_id {
            return Err(FederationError::NotAuthoritative);
        }
        lease.granted_at = now;
        Ok(*lease)
    }

    pub fn release_lease(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        agent_instance: u64,
    ) -> Result<(), FederationError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        self.leases.lock().unwrap().remove(&agent_instance);
        Ok(())
    }

    pub fn lease_of(&self, agent_instance: u64) -> Option<AnchorLease> {
        self.leases.lock().unwrap().get(&agent_instance).copied()
    }

    /// Starts a real background thread that renews `agent_instance`'s lease for `device_id` every
    /// real `interval` — the "heartbeat timing" half of this crate's own previously-named "real
    /// network transport, heartbeat timing, ambient anti-entropy" gap. Uses the real system clock
    /// (`SystemTime::now`), unlike every other method on this hub, which takes a caller-supplied
    /// logical `now`: a heartbeat is inherently an autonomous, real-time background behavior, not
    /// a deterministic step a test drives directly. A renewal that fails (e.g. the lease expired
    /// and was reclaimed by another device before this tick ran) is silently skipped rather than
    /// panicking a background thread — the next tick tries again regardless; a caller that cares
    /// about the outcome can still call [`Self::lease_of`]/[`Self::renew_lease`] directly.
    /// Requires `self` already be held in an `Arc` (the thread holds its own clone) — this hub
    /// itself is otherwise unaware whether any heartbeat is running for a given lease, matching
    /// [`Self::establish_shared_secret`]'s own "recomputed, never cached" simplicity.
    pub fn start_lease_heartbeat(
        self: &Arc<Self>,
        monitor: Arc<CapabilityMonitor>,
        token: CapabilityToken,
        agent_instance: u64,
        device_id: u64,
        interval: Duration,
    ) -> LeaseHeartbeat {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let hub = Arc::clone(self);
        let handle = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                std::thread::sleep(interval);
                if thread_stop.load(Ordering::Relaxed) {
                    break;
                }
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("system clock before Unix epoch")
                    .as_secs();
                let _ = hub.renew_lease(&monitor, &token, agent_instance, device_id, now);
            }
        });
        LeaseHeartbeat {
            stop,
            handle: Some(handle),
        }
    }

    /// Spawns a real Agent on `device_id`'s own `AgentRuntime`, mints a
    /// global identity for it (each device's own instance counter is
    /// independent, so a bare local id would collide across devices), and
    /// grants it a fresh `AnchorLease` held by the spawning device.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_agent(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        device_id: u64,
        manifest: AgentManifest,
        bound_intent: Option<u64>,
        now: u64,
        lease_ttl_secs: u64,
    ) -> Result<u64, FederationError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let runtime = self.device(device_id)?;
        let local_instance = runtime.spawn(monitor, token, manifest, bound_intent)?;

        let global_id = self.next_agent_id.fetch_add(1, Ordering::Relaxed);
        self.agents.lock().unwrap().insert(
            global_id,
            AgentRef {
                device_id,
                local_instance,
            },
        );
        self.leases.lock().unwrap().insert(
            global_id,
            AnchorLease {
                agent_instance: global_id,
                holder_device: device_id,
                generation: 0,
                granted_at: now,
                ttl_secs: lease_ttl_secs,
            },
        );
        Ok(global_id)
    }

    /// `triggering_intent_id` is a caller-supplied real `hyperion-intent`
    /// Intent `NodeId.0` — see [`Self::dispatch_offload`]'s doc comment on
    /// why this crate doesn't itself depend on `hyperion-intent` to get one.
    #[allow(clippy::too_many_arguments)]
    pub fn invoke_agent(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        global_agent_id: u64,
        capability_ref: &str,
        args: serde_json::Value,
        triggering_intent_id: u64,
        now: u64,
    ) -> Result<InvokeOutcome, FederationError> {
        self.require(monitor, token, RightsMask::EXEC)?;
        let agent_ref = *self
            .agents
            .lock()
            .unwrap()
            .get(&global_agent_id)
            .ok_or(FederationError::NoSuchAgent)?;
        let runtime = self.device(agent_ref.device_id)?;

        let action_id = self.explanations.next_action_id();
        let explanation_id = self.explanations.begin(
            monitor,
            token,
            action_id,
            triggering_intent_id,
            global_agent_id,
            capability_ref,
            vec![],
            now,
        )?;
        self.explanations.append_step(
            monitor,
            token,
            explanation_id,
            ReasoningStep {
                step_index: 0,
                description: format!(
                    "invoked global agent {global_agent_id} on device {}",
                    agent_ref.device_id
                ),
                capability_ref: Some(capability_ref.to_string()),
                inputs_ref: Vec::new(),
                output_ref: None,
            },
            Vec::new(),
        )?;
        self.explanations
            .transition(monitor, token, explanation_id, ControlState::Executing)?;

        let outcome = runtime.invoke(
            monitor,
            token,
            agent_ref.local_instance,
            capability_ref,
            args,
        )?;
        self.explanations.transition(
            monitor,
            token,
            explanation_id,
            match &outcome {
                InvokeOutcome::Result(_) => ControlState::Completed,
                InvokeOutcome::PendingConsent | InvokeOutcome::QuotaExceeded => {
                    ControlState::Interrupted
                }
                InvokeOutcome::Denied | InvokeOutcome::Failed(_) => ControlState::RolledBack,
            },
        )?;
        Ok(outcome)
    }

    pub fn device_of(&self, global_agent_id: u64) -> Option<u64> {
        self.agents
            .lock()
            .unwrap()
            .get(&global_agent_id)
            .map(|r| r.device_id)
    }

    /// docs/21 §Algorithms' "Session/state migration": freeze via
    /// checkpoint, transfer the checkpoint's contents, spawn-and-rebind on
    /// the target (this crate's cross-runtime analogue of `resume`, since
    /// [`hyperion_agent_runtime::AgentRuntime::resume`] only continues an
    /// instance record within its own runtime), hand off the lease, and
    /// terminate the source instance with reason `"migrated"` — the same
    /// six steps the doc specifies, five of them literally reused from
    /// `hyperion-agent-runtime`. Also the real production call site for
    /// `hyperion_observability::TelemetryCollector::merge_remote_trace`:
    /// whatever a caller recorded on the source device's collector under
    /// `trace_id` is pulled into the target device's collector before the
    /// source instance is torn down, so continuing to query the target's
    /// telemetry after the hop reconstructs the whole cross-device trace,
    /// not just what ran there after migration.
    #[allow(clippy::too_many_arguments)]
    pub fn migrate(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        global_agent_id: u64,
        target_device_id: u64,
        trace_id: TraceId,
        now: u64,
        lease_ttl_secs: u64,
    ) -> Result<MigrationReceipt, FederationError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let migration_id = self.next_migration_id.fetch_add(1, Ordering::Relaxed);

        let agent_ref = *self
            .agents
            .lock()
            .unwrap()
            .get(&global_agent_id)
            .ok_or(FederationError::NoSuchAgent)?;

        let lease = self
            .leases
            .lock()
            .unwrap()
            .get(&global_agent_id)
            .copied()
            .ok_or(FederationError::NoSuchLease)?;
        if lease.holder_device != agent_ref.device_id {
            return Err(FederationError::NotAuthoritative); // only the current anchor may initiate
        }

        let source_runtime = self.device(agent_ref.device_id)?;
        let target_runtime = self.device(target_device_id)?;

        let checkpoint_id = source_runtime.checkpoint(monitor, token, agent_ref.local_instance)?;
        let checkpoint = source_runtime
            .get_checkpoint(checkpoint_id)
            .expect("checkpoint() always stores what it just created");

        let new_local_instance = target_runtime.spawn(
            monitor,
            token,
            checkpoint.manifest.clone(),
            checkpoint.bound_intent,
        )?;

        if let (Some(source_telemetry), Some(target_telemetry)) = (
            self.telemetry_for(agent_ref.device_id),
            self.telemetry_for(target_device_id),
        ) {
            target_telemetry.merge_remote_trace(trace_id, &source_telemetry);
        }

        source_runtime.terminate(monitor, token, agent_ref.local_instance, "migrated")?;

        self.agents.lock().unwrap().insert(
            global_agent_id,
            AgentRef {
                device_id: target_device_id,
                local_instance: new_local_instance,
            },
        );
        self.leases.lock().unwrap().insert(
            global_agent_id,
            AnchorLease {
                agent_instance: global_agent_id,
                holder_device: target_device_id,
                generation: lease.generation + 1,
                granted_at: now,
                ttl_secs: lease_ttl_secs,
            },
        );

        Ok(MigrationReceipt {
            migration_id,
            agent_instance: global_agent_id,
            target_device: target_device_id,
            outcome: MigrationOutcome::Completed,
        })
    }
}
