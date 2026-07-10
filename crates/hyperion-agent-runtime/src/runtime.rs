use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};

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
}

impl Default for AgentRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentRuntime {
    pub fn new() -> Self {
        AgentRuntime {
            instances: Mutex::new(HashMap::new()),
            checkpoints: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
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

        if !instance.quota.has_headroom() {
            Self::audit(instance, "quota_exceeded", capability_ref);
            return Ok(InvokeOutcome::QuotaExceeded);
        }

        instance.state = LifecycleState::Executing;
        instance.quota.calls_used_this_window += 1;

        match stubs::dispatch(capability_ref, &args) {
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
