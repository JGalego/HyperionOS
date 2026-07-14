use std::collections::HashMap;
use std::sync::Mutex;

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_crypto::{Keystore, Signature, VerifyingKey};
use hyperion_recovery::{RecoveryPointId, RecoveryService, Trigger};

use crate::types::{
    CompatibilityCheckResult, RollbackReceipt, RolloutState, UpdateError, UpdateManifest,
    UpdateSubject, Version,
};

/// The exact fields a real signature is produced/verified over — the same fields the
/// non-cryptographic-checksum stand-in this replaces already chose to cover (`subject` via its
/// `Debug` string, since `UpdateSubject` has no other stable byte form; `from_version`/
/// `to_version`/`touched_objects` directly), now signed instead of hashed with `DefaultHasher`
/// (SipHash, explicitly not cryptographic and not stable across Rust releases — never suitable
/// for this even as a stand-in's *hashing* choice, let alone a signature's).
fn canonical_bytes(manifest_without_signature: &UpdateManifest) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(format!("{:?}", manifest_without_signature.subject).as_bytes());
    bytes.extend_from_slice(&manifest_without_signature.from_version.to_le_bytes());
    bytes.extend_from_slice(&manifest_without_signature.to_version.to_le_bytes());
    for node_id in &manifest_without_signature.touched_objects {
        bytes.extend_from_slice(&node_id.0.to_le_bytes());
    }
    bytes
}

/// A real Ed25519 signature over `manifest_without_signature`'s own canonical bytes
/// (docs/998-roadmap.md M9) — the value a caller populates [`UpdateManifest::signature`]
/// with before [`UpdateOrchestrator::apply_update`]. See [`hyperion_crypto`]'s own doc comment on
/// why this workspace verifies against one real, trusted device identity rather than a
/// multi-publisher trust store docs/32's own "verified per 15" framing would otherwise imply.
pub fn sign(manifest_without_signature: &UpdateManifest, keystore: &Keystore) -> Signature {
    keystore.sign(&canonical_bytes(manifest_without_signature))
}

fn verify_signature(manifest: &UpdateManifest, verifying_key: &VerifyingKey) -> bool {
    let mut unsigned = manifest.clone();
    unsigned.signature = None;
    match &manifest.signature {
        Some(signature) => {
            hyperion_crypto::verify(&canonical_bytes(&unsigned), signature, verifying_key)
        }
        None => false,
    }
}

/// docs/32 — the Update System's System-Image/Capability/Model tracks.
/// See this crate's doc comment for the full real/deferred split.
pub struct UpdateOrchestrator {
    recovery: std::sync::Arc<RecoveryService>,
    active_versions: Mutex<HashMap<UpdateSubject, Version>>,
    recovery_points: Mutex<HashMap<UpdateSubject, RecoveryPointId>>,
    rollout_states: Mutex<HashMap<UpdateSubject, RolloutState>>,
}

impl UpdateOrchestrator {
    pub fn new(recovery: std::sync::Arc<RecoveryService>) -> Self {
        UpdateOrchestrator {
            recovery,
            active_versions: Mutex::new(HashMap::new()),
            recovery_points: Mutex::new(HashMap::new()),
            rollout_states: Mutex::new(HashMap::new()),
        }
    }

    fn require(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        rights: RightsMask,
    ) -> Result<(), UpdateError> {
        monitor
            .check_rights_ok_result(token, rights)
            .map_err(|_| UpdateError::Unauthorized)
    }

    pub fn active_version(&self, subject: &UpdateSubject) -> Version {
        self.active_versions
            .lock()
            .unwrap()
            .get(subject)
            .copied()
            .unwrap_or(0)
    }

    pub fn rollout_state(&self, subject: &UpdateSubject) -> Option<RolloutState> {
        self.rollout_states.lock().unwrap().get(subject).copied()
    }

    /// docs/32 §2's `compatibility_check`, narrowed to schema + hardware
    /// (no dependency graph exists to populate `blocking_dependencies`).
    /// Schema compatibility is exactly "this update's `from_version`
    /// matches what's currently active" — an update built against a
    /// version that has since moved is rejected, never silently applied
    /// on top of the wrong base.
    pub fn compatibility_check(
        &self,
        manifest: &UpdateManifest,
        hardware_compatible: bool,
    ) -> CompatibilityCheckResult {
        CompatibilityCheckResult {
            schema_compatible: manifest.from_version == self.active_version(&manifest.subject),
            migration_required: !manifest.touched_objects.is_empty(),
            hardware_compatible,
        }
    }

    /// docs/32 §1's `apply_update` pipeline: verify signature →
    /// compatibility check → `recovery_point_create(PreUpdate)` →
    /// staged, health-gated rollout, monotonically advancing through
    /// `manifest.rollout_policy.stages` — never time-gated alone. A
    /// caller supplies `health_for_stage` since this crate has no real
    /// fleet telemetry to poll (see this crate's doc comment); it is
    /// called once per stage with that stage's `percent`.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_update(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        manifest: &UpdateManifest,
        hardware_compatible: bool,
        now: u64,
        mut health_for_stage: impl FnMut(u8) -> crate::types::CohortHealth,
        verifying_key: &VerifyingKey,
    ) -> Result<Version, UpdateError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        if !verify_signature(manifest, verifying_key) {
            return Err(UpdateError::SignatureInvalid);
        }

        let compat = self.compatibility_check(manifest, hardware_compatible);
        if !compat.is_compatible() {
            return Err(UpdateError::Incompatible);
        }

        self.rollout_states
            .lock()
            .unwrap()
            .insert(manifest.subject.clone(), RolloutState::Staged);

        let recovery_point = self.recovery.recovery_point_create(
            monitor,
            token,
            Trigger::PreUpdate,
            &manifest.touched_objects,
            now,
        )?;
        self.recovery_points
            .lock()
            .unwrap()
            .insert(manifest.subject.clone(), recovery_point);

        for (stage_index, stage) in manifest.rollout_policy.stages.iter().enumerate() {
            self.rollout_states.lock().unwrap().insert(
                manifest.subject.clone(),
                RolloutState::Canary { stage_index },
            );

            let health = health_for_stage(stage.percent);
            if !health.within(&manifest.rollout_policy.health_thresholds) {
                if manifest.rollout_policy.auto_rollback_on_breach {
                    self.update_rollback(monitor, token, manifest)?;
                } else {
                    self.rollout_states
                        .lock()
                        .unwrap()
                        .insert(manifest.subject.clone(), RolloutState::RolledBack);
                }
                return Err(UpdateError::RolloutHealthBreach);
            }
        }

        self.active_versions
            .lock()
            .unwrap()
            .insert(manifest.subject.clone(), manifest.to_version);
        self.rollout_states
            .lock()
            .unwrap()
            .insert(manifest.subject.clone(), RolloutState::RolledOut);
        Ok(manifest.to_version)
    }

    /// docs/32 §5's `update_rollback(subject, to_version) -> RollbackReceipt
    /// // delegates to 33` — the literal relationship docs/32 states:
    /// "[33] is what actually makes an update reversible." Restores
    /// every object the pre-update recovery point captured via
    /// `hyperion_recovery::RecoveryService::restore_to`, then moves the
    /// active-version pointer back — callable both mid-rollout (from
    /// [`Self::apply_update`]'s own health-breach path) and post-hoc,
    /// against an already-`RolledOut` subject.
    pub fn update_rollback(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        manifest: &UpdateManifest,
    ) -> Result<RollbackReceipt, UpdateError> {
        self.require(monitor, token, RightsMask::WRITE)?;

        let recovery_point = self
            .recovery_points
            .lock()
            .unwrap()
            .get(&manifest.subject)
            .copied()
            .ok_or(UpdateError::NoRecoveryPoint)?;
        if !manifest.touched_objects.is_empty() {
            self.recovery.restore_to(monitor, token, recovery_point)?;
        }

        self.active_versions
            .lock()
            .unwrap()
            .insert(manifest.subject.clone(), manifest.from_version);
        self.rollout_states
            .lock()
            .unwrap()
            .insert(manifest.subject.clone(), RolloutState::RolledBack);
        Ok(RollbackReceipt {
            subject: manifest.subject.clone(),
            rolled_back_to: manifest.from_version,
        })
    }
}
