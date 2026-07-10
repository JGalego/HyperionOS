use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_scheduler::{
    AgentId, IntentId, ResourceDimension, ResourceLedger, ResourceVector, SchedClass, Scheduler,
    TaskDescriptor, TaskId,
};

use crate::broker::{self, GrantDecision};
use crate::stubs;
use crate::types::{
    AgentCheckpoint, AgentInstance, AgentManifest, AuditEntry, CapabilityGrant, InvokeOutcome,
    LifecycleState, QuotaState,
};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs()
}

/// docs/11 §6.2: "a circuit breaker trips after N consecutive Capability
/// failures within one window."
const CIRCUIT_BREAKER_THRESHOLD: u32 = 3;
const DEFAULT_QUOTA: u32 = 100;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("no such agent instance")]
    NotFound,
    #[error("invalid state transition: {0}")]
    InvalidState(String),
}

/// docs/11 — Agent Runtime. See this crate's doc comment for what's
/// deferred.
pub struct AgentRuntime {
    instances: Mutex<HashMap<u64, AgentInstance>>,
    checkpoints: Mutex<HashMap<u64, AgentCheckpoint>>,
    next_id: AtomicU64,
    /// docs/04's real unified Scheduler, backing [`Self::invoke`]'s quota
    /// gate — see this crate's doc comment on why admission is delegated
    /// here instead of `QuotaState`'s own private counter.
    scheduler: Mutex<Scheduler>,
    next_task_id: AtomicU64,
}

impl Default for AgentRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentRuntime {
    pub fn new() -> Self {
        let mut scheduler = Scheduler::new();
        // One nominal dimension stands in for "a Capability invocation's
        // resource footprint" — `DEFAULT_QUOTA` reused as the ledger's
        // capacity keeps this the same number `QuotaState` always used,
        // just enforced by the real admission algorithm instead of a
        // private counter.
        scheduler.register_resource_provider(ResourceLedger::new(
            ResourceDimension::InferenceTokens,
            DEFAULT_QUOTA,
            0,
        ));
        AgentRuntime {
            instances: Mutex::new(HashMap::new()),
            checkpoints: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            scheduler: Mutex::new(scheduler),
            next_task_id: AtomicU64::new(1),
        }
    }

    /// Real headroom remaining on the Scheduler's single
    /// `InferenceTokens` ledger this runtime's Capability invocations
    /// draw from — queryable proof that [`Self::invoke`] round-trips
    /// through the real admission algorithm rather than a private
    /// counter: it reads `DEFAULT_QUOTA` before any call and after every
    /// call, since each invocation's resource request is released the
    /// moment its (synchronous, in this simulator) dispatch finishes.
    pub fn resource_headroom(&self) -> u32 {
        self.scheduler
            .lock()
            .unwrap()
            .query_ledger(ResourceDimension::InferenceTokens)
            .map(|l| l.headroom(false))
            .unwrap_or(0)
    }

    fn require(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        rights: RightsMask,
    ) -> Result<(), AgentError> {
        monitor
            .check_rights_ok_result(token, rights)
            .map_err(|_| AgentError::Unauthorized)
    }

    fn audit(instance: &mut AgentInstance, kind: &str, detail: impl Into<String>) {
        instance.audit_log.push(AuditEntry {
            timestamp: now(),
            kind: kind.to_string(),
            detail: detail.into(),
        });
    }

    /// `AgentRuntime.spawn` fused with `bind` — docs/11 §7's signature
    /// already takes `intent_ref`/`context_bundle_ref` at spawn time, so
    /// this crate does not model a separate unbound `spawning` state
    /// observable to callers.
    pub fn spawn(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        manifest: AgentManifest,
        bound_intent: Option<u64>,
    ) -> Result<u64, AgentError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let instance_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut instance = AgentInstance {
            instance_id,
            manifest,
            state: LifecycleState::Bound,
            bound_intent,
            grants: Vec::new(),
            quota: QuotaState::new(DEFAULT_QUOTA),
            pending_consent: None,
            audit_log: Vec::new(),
        };
        Self::audit(&mut instance, "bound", format!("intent={bound_intent:?}"));
        self.instances.lock().unwrap().insert(instance_id, instance);
        Ok(instance_id)
    }

    /// docs/11 §7's `invoke` — routed through the Broker (§6.1) and quota/
    /// circuit breaker (§6.2), then dispatched to a stub Capability.
    pub fn invoke(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        instance_id: u64,
        capability_ref: &str,
        args: serde_json::Value,
    ) -> Result<InvokeOutcome, AgentError> {
        self.require(monitor, token, RightsMask::EXEC)?;

        let mut instances = self.instances.lock().unwrap();
        let instance = instances
            .get_mut(&instance_id)
            .ok_or(AgentError::NotFound)?;

        match instance.state {
            LifecycleState::Terminated
            | LifecycleState::Completed
            | LifecycleState::Failed
            | LifecycleState::Suspended => {
                return Err(AgentError::InvalidState(format!(
                    "cannot invoke while {:?}",
                    instance.state
                )));
            }
            _ => {}
        }

        match broker::resolve_grant(instance, capability_ref) {
            GrantDecision::Denied => {
                Self::audit(instance, "denied", capability_ref);
                return Ok(InvokeOutcome::Denied);
            }
            GrantDecision::PendingConsent => {
                instance.state = LifecycleState::WaitingOnCapability;
                instance.pending_consent = Some(capability_ref.to_string());
                Self::audit(instance, "pending_consent", capability_ref);
                return Ok(InvokeOutcome::PendingConsent);
            }
            GrantDecision::Granted => {}
        }

        // docs/04's real Scheduler admission gate, replacing a private
        // `QuotaState.has_headroom()` counter that never touched the rest
        // of the system's real resource model. `QuotaState` itself is
        // still updated below for the circuit-breaker's own bookkeeping
        // (unrelated to this gate) and observability.
        let task_id = TaskId(self.next_task_id.fetch_add(1, Ordering::Relaxed));
        let ticket = self
            .scheduler
            .lock()
            .unwrap()
            .submit_task(
                monitor,
                TaskDescriptor {
                    id: task_id,
                    owner_intent: IntentId(instance.bound_intent.unwrap_or(0)),
                    owner_agent: Some(AgentId(instance_id)),
                    class: SchedClass::InteractiveAgent,
                    deadline: None,
                    priority_weight: 1.0,
                    request: ResourceVector {
                        inference_tokens_per_sec: 1,
                        ..Default::default()
                    },
                    cap_token: token.clone(),
                },
            )
            .map_err(|_| AgentError::Unauthorized)?;
        let admitted = self
            .scheduler
            .lock()
            .unwrap()
            .schedule_epoch()
            .into_iter()
            .find(|r| r.ticket == ticket)
            .map(|r| r.admitted)
            .unwrap_or(false);
        if !admitted {
            let _ = self.scheduler.lock().unwrap().cancel(ticket);
            Self::audit(instance, "quota_exceeded", capability_ref);
            return Ok(InvokeOutcome::QuotaExceeded);
        }

        instance.state = LifecycleState::Executing;
        instance.quota.calls_used_this_window += 1;

        let dispatch_result = stubs::dispatch(capability_ref, &args);
        let _ = self.scheduler.lock().unwrap().complete(ticket);

        match dispatch_result {
            Ok(result) => {
                instance.quota.consecutive_failures = 0;
                Self::audit(instance, "invoked", capability_ref);
                Ok(InvokeOutcome::Result(result))
            }
            Err(reason) => {
                instance.quota.consecutive_failures += 1;
                Self::audit(
                    instance,
                    "capability_failed",
                    format!("{capability_ref}: {reason}"),
                );
                if instance.quota.consecutive_failures >= CIRCUIT_BREAKER_THRESHOLD {
                    instance.state = LifecycleState::Suspended;
                    Self::audit(instance, "suspended_runaway", capability_ref);
                }
                Ok(InvokeOutcome::Failed(reason))
            }
        }
    }

    /// docs/11 §6.1: the consent round trip's resolution — see this
    /// crate's doc comment on the deferred real UI prompt.
    pub fn resolve_consent(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        instance_id: u64,
        approved: bool,
    ) -> Result<(), AgentError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let mut instances = self.instances.lock().unwrap();
        let instance = instances
            .get_mut(&instance_id)
            .ok_or(AgentError::NotFound)?;
        let Some(capability_ref) = instance.pending_consent.take() else {
            return Err(AgentError::InvalidState(
                "no pending consent request".to_string(),
            ));
        };

        if approved {
            instance.grants.push(CapabilityGrant {
                capability_ref: capability_ref.clone(),
                scope: Vec::new(),
                granted_at: now(),
            });
            Self::audit(instance, "consent_granted", capability_ref);
        } else {
            Self::audit(instance, "consent_denied", capability_ref);
        }
        instance.state = LifecycleState::Executing;
        Ok(())
    }

    /// docs/11 §6.3: serializes the manifest and bound Intent reference,
    /// revokes open grants (never carried across — resume re-requests
    /// them), and tears down.
    pub fn checkpoint(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        instance_id: u64,
    ) -> Result<u64, AgentError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let mut instances = self.instances.lock().unwrap();
        let instance = instances
            .get_mut(&instance_id)
            .ok_or(AgentError::NotFound)?;

        let checkpoint_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let checkpoint = AgentCheckpoint {
            checkpoint_id,
            instance_id,
            manifest: instance.manifest.clone(),
            bound_intent: instance.bound_intent,
            created_at: now(),
        };
        instance.grants.clear();
        instance.state = LifecycleState::Checkpointed;
        Self::audit(instance, "checkpointed", checkpoint_id.to_string());
        self.checkpoints
            .lock()
            .unwrap()
            .insert(checkpoint_id, checkpoint);
        Ok(checkpoint_id)
    }

    /// docs/11 §6.3: re-binds the same Intent, rehydrates state, returns to
    /// `executing` — grants must be re-requested, per checkpoint's revoke.
    pub fn resume(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        checkpoint_id: u64,
    ) -> Result<u64, AgentError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let checkpoint = self
            .checkpoints
            .lock()
            .unwrap()
            .get(&checkpoint_id)
            .cloned()
            .ok_or(AgentError::NotFound)?;

        let mut instances = self.instances.lock().unwrap();
        let instance = instances
            .get_mut(&checkpoint.instance_id)
            .ok_or(AgentError::NotFound)?;
        if instance.state != LifecycleState::Checkpointed {
            return Err(AgentError::InvalidState(format!(
                "cannot resume from {:?}",
                instance.state
            )));
        }
        instance.state = LifecycleState::Executing;
        Self::audit(instance, "resumed", checkpoint_id.to_string());
        Ok(checkpoint.instance_id)
    }

    /// Exposes a checkpoint's contents (manifest, bound Intent reference)
    /// so a caller orchestrating *across* two `AgentRuntime` instances —
    /// `hyperion-federation`'s cross-device migration is the motivating
    /// case — can transfer it, since [`Self::resume`] only ever continues
    /// an instance record within this same runtime.
    pub fn get_checkpoint(&self, checkpoint_id: u64) -> Option<AgentCheckpoint> {
        self.checkpoints
            .lock()
            .unwrap()
            .get(&checkpoint_id)
            .cloned()
    }

    pub fn terminate(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        instance_id: u64,
        reason: &str,
    ) -> Result<(), AgentError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let mut instances = self.instances.lock().unwrap();
        let instance = instances
            .get_mut(&instance_id)
            .ok_or(AgentError::NotFound)?;
        instance.state = LifecycleState::Terminated;
        Self::audit(instance, "terminated", reason);
        Ok(())
    }

    pub fn mark_completed(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        instance_id: u64,
    ) -> Result<(), AgentError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let mut instances = self.instances.lock().unwrap();
        let instance = instances
            .get_mut(&instance_id)
            .ok_or(AgentError::NotFound)?;
        instance.state = LifecycleState::Completed;
        Self::audit(instance, "completed", "");
        Ok(())
    }

    pub fn describe(&self, instance_id: u64) -> Option<AgentInstance> {
        self.instances.lock().unwrap().get(&instance_id).cloned()
    }

    pub fn state_of(&self, instance_id: u64) -> Option<LifecycleState> {
        self.instances
            .lock()
            .unwrap()
            .get(&instance_id)
            .map(|i| i.state)
    }
}
