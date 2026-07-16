use hyperion_knowledge_graph::NodeId;

/// A flat monotonic version number, standing in for docs/32's `SemVer` —
/// this workspace has no semver-parsing dependency, and every ordering
/// decision this crate makes (`from_version`/`to_version` comparison)
/// only needs a total order, not a real semver spec.
pub type Version = u32;

/// docs/32 §2's `UpdateSubject`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum UpdateSubject {
    SystemImage,
    Capability { id: String },
    Model { id: String },
}

/// docs/32 §2's `CompatibilityCheckResult`, narrowed to the two checks
/// this crate can actually evaluate — no dependency graph exists to
/// populate `blocking_dependencies`.
#[derive(Debug, Clone, Copy)]
pub struct CompatibilityCheckResult {
    pub schema_compatible: bool,
    pub migration_required: bool,
    pub hardware_compatible: bool,
}

impl CompatibilityCheckResult {
    pub fn is_compatible(&self) -> bool {
        self.schema_compatible && self.hardware_compatible
    }
}

/// docs/32 §1's health-gate inputs, docs/34-shaped
/// (`observability::cohort_health`) but caller-supplied here — this
/// crate has no fleet telemetry to poll, so a caller passes a
/// `CohortHealth` per stage exactly as it would read one off a real
/// dashboard.
#[derive(Debug, Clone, Copy)]
pub struct CohortHealth {
    pub crash_rate: f32,
    pub latency_p99_ms: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct HealthThresholds {
    pub max_crash_rate: f32,
    pub max_latency_p99_ms: u32,
}

impl CohortHealth {
    pub fn within(&self, thresholds: &HealthThresholds) -> bool {
        self.crash_rate <= thresholds.max_crash_rate
            && self.latency_p99_ms <= thresholds.max_latency_p99_ms
    }
}

/// docs/32 §1's `CohortStage` — a percentage of the fleet, not a real
/// device population (see this crate's doc comment).
#[derive(Debug, Clone, Copy)]
pub struct CohortStage {
    pub percent: u8,
}

/// docs/32 §2's `RolloutPolicy`.
#[derive(Debug, Clone)]
pub struct RolloutPolicy {
    pub stages: Vec<CohortStage>,
    pub health_thresholds: HealthThresholds,
    pub auto_rollback_on_breach: bool,
}

impl RolloutPolicy {
    /// docs/32 §1's default stage schedule: `[1%, 10%, 50%, 100%]`.
    pub fn default_schedule(health_thresholds: HealthThresholds) -> Self {
        RolloutPolicy {
            stages: vec![
                CohortStage { percent: 1 },
                CohortStage { percent: 10 },
                CohortStage { percent: 50 },
                CohortStage { percent: 100 },
            ],
            health_thresholds,
            auto_rollback_on_breach: true,
        }
    }
}

/// docs/32 §2's `UpdateManifest`, narrowed: `migration_plan` becomes
/// `touched_objects` — exactly what
/// `hyperion_recovery::RecoveryService::recovery_point_create` needs to
/// snapshot, since this crate has no separate expand/contract migration
/// DSL (see this crate's doc comment). `signature` (docs/998-roadmap.md M9) is a real
/// Ed25519 signature over [`crate::orchestrator::sign`]'s canonical bytes — see that function's
/// own doc comment on this workspace's single-device-identity model.
#[derive(Debug, Clone)]
pub struct UpdateManifest {
    pub subject: UpdateSubject,
    pub from_version: Version,
    pub to_version: Version,
    pub signature: Option<hyperion_crypto::Signature>,
    pub touched_objects: Vec<NodeId>,
    pub rollout_policy: RolloutPolicy,
}

/// docs/32 §2's `RolloutState`, `Canary{stage_index, started_at}`
/// narrowed to drop `started_at` — this crate has no real clock to time
/// a soak period against (a caller drives staging synchronously).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RolloutState {
    Staged,
    Canary { stage_index: usize },
    RolledOut,
    RolledBack,
}

/// docs/32 §5's `RollbackReceipt`.
#[derive(Debug, Clone)]
pub struct RollbackReceipt {
    pub subject: UpdateSubject,
    pub rolled_back_to: Version,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemImageSlotName {
    A,
    B,
}

/// docs/32 §2's `SystemImageSlot`.
#[derive(Debug, Clone, Copy)]
pub struct SystemImageSlot {
    pub slot: SystemImageSlotName,
    pub version: Version,
    pub boot_attempts_remaining: u8,
    pub committed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("manifest signature does not verify")]
    SignatureInvalid,
    #[error("update is incompatible with the current schema/hardware")]
    Incompatible,
    #[error("rollout health breached its thresholds")]
    RolloutHealthBreach,
    #[error("no recovery point is on record for this subject")]
    NoRecoveryPoint,
    #[error("this slot has exhausted its boot attempts")]
    BootAttemptsExhausted,
    #[error(
        "refusing to retry: this exact update (subject, from_version, to_version) already \
         rolled back once for this reason: {reason}"
    )]
    RepeatedRecentRollback { reason: String },
    #[error("recovery error: {0}")]
    Recovery(#[from] hyperion_recovery::RecoveryError),
    #[error("model router error: {0}")]
    ModelRouter(#[from] hyperion_model_router::ModelRouterError),
    #[error(
        "refusing to stage version {attempted} through the normal update path: it is not newer \
         than {highest_ever} (the highest version ever installed) -- a deliberate downgrade must \
         go through the explicit, audited rollback path instead"
    )]
    AntiRollbackViolation {
        attempted: Version,
        highest_ever: Version,
    },
}
