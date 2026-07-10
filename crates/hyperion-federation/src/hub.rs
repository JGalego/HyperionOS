use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hyperion_agent_runtime::{AgentManifest, AgentRuntime, InvokeOutcome};
use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_explainability::{
    ControlState, ExplanationId, ExplanationRecord, ExplanationStore, ReasoningStep,
};
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
    /// and [`Self::explanation`]/[`Self::trace_intent`].
    explanations: ExplanationStore,
    next_action_id: AtomicU64,
}

impl Default for FederationHub {
    fn default() -> Self {
        Self::new()
    }
}

impl FederationHub {
    pub fn new() -> Self {
        FederationHub {
            devices: Mutex::new(HashMap::new()),
            trust_tiers: Mutex::new(HashMap::new()),
            ledgers: Mutex::new(HashMap::new()),
            leases: Mutex::new(HashMap::new()),
            agents: Mutex::new(HashMap::new()),
            next_agent_id: AtomicU64::new(1),
            next_migration_id: AtomicU64::new(1),
            explanations: ExplanationStore::new(),
            next_action_id: AtomicU64::new(1),
        }
    }

    /// docs/18's "queryable Explanation Record" surface for this hub's
    /// own dispatches — see [`Self::dispatch_offload`]/[`Self::invoke_agent`].
    pub fn explanation(&self, id: ExplanationId) -> Option<ExplanationRecord> {
        self.explanations.get(id)
    }

    /// Every record this hub has opened under `intent_id` — federation
    /// dispatches don't yet carry a real originating Intent id (no
    /// concept exists at this layer), so every call currently records
    /// under the sentinel `intent_id = 0`; see this crate's doc comment.
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
    /// `AgentRuntime` instance — per device.
    pub fn join_device(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        device_id: u64,
        trust_tier: FederationTrustTier,
    ) -> Result<(), FederationError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        self.devices
            .lock()
            .unwrap()
            .insert(device_id, Arc::new(AgentRuntime::new()));
        self.trust_tiers
            .lock()
            .unwrap()
            .insert(device_id, trust_tier);
        Ok(())
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
    /// next one, matching the doc's own retry loop.
    pub fn dispatch_offload(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        descriptor: &OffloadDescriptor,
        capability_ref: &str,
        args: serde_json::Value,
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

            let action_id = self.next_action_id.fetch_add(1, Ordering::Relaxed);
            let explanation_id = self.explanations.begin(
                monitor,
                token,
                action_id,
                0,
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

    pub fn invoke_agent(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        global_agent_id: u64,
        capability_ref: &str,
        args: serde_json::Value,
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

        let action_id = self.next_action_id.fetch_add(1, Ordering::Relaxed);
        let explanation_id = self.explanations.begin(
            monitor,
            token,
            action_id,
            0,
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
    /// `hyperion-agent-runtime`.
    pub fn migrate(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        global_agent_id: u64,
        target_device_id: u64,
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
