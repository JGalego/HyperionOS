use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_crypto::VerifyingKey;

use crate::registry::{verify, ModelRegistry};
use crate::residency::ResidencyManager;
use crate::types::{
    CapabilityContract, InferenceRequest, InferenceResult, ModelClass, ModelDescriptor, PowerMode,
    QuantizedVariant, ResidencyEntry, ResourceEstimate,
};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs()
}

/// A pluggable execution backend — see this crate's doc comment. Swapping
/// [`crate::MockBackend`] for a real one is the entire integration surface
/// a future session needs to touch to run real models.
pub trait InferenceBackend: Send + Sync {
    fn generate(&self, model_id: u64, request: &InferenceRequest) -> String;
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("no model registered for this class meets the capability contract on this hardware")]
    InfeasibleLocally,
    #[error("model artifact failed integrity verification")]
    IntegrityFailure,
    #[error("no such model registered")]
    NotFound,
}

/// docs/22 — Local AI Runtime. See this crate's doc comment for what's
/// deferred (real model execution, real scheduler-governor wiring, real
/// streaming/cancellation, real signing).
pub struct LocalAiRuntime {
    registry: Mutex<ModelRegistry>,
    residency: Mutex<ResidencyManager>,
    power_mode: Mutex<PowerMode>,
    backend: Box<dyn InferenceBackend>,
    total_capacity_mb: u32,
    next_request_id: AtomicU64,
}

impl LocalAiRuntime {
    pub fn new(backend: Box<dyn InferenceBackend>, total_capacity_mb: u32) -> Self {
        LocalAiRuntime {
            registry: Mutex::new(ModelRegistry::default()),
            residency: Mutex::new(ResidencyManager::default()),
            power_mode: Mutex::new(PowerMode::Performance),
            backend,
            total_capacity_mb,
            next_request_id: AtomicU64::new(1),
        }
    }

    /// Registers a model artifact after checking its real Ed25519 signature — docs/22
    /// §Security Considerations: "every model artifact is signature-verified... before load."
    /// Rejects (rather than silently accepting) a tampered, unsigned, or forged descriptor.
    /// `verifying_key` is the real public key the caller trusts to have signed this descriptor —
    /// see [`hyperion_crypto::Keystore`]'s own doc comment on this workspace's single-device-
    /// identity model.
    pub fn register_model(
        &self,
        descriptor: ModelDescriptor,
        verifying_key: &VerifyingKey,
    ) -> Result<(), RuntimeError> {
        if !verify(&descriptor, verifying_key) {
            return Err(RuntimeError::IntegrityFailure);
        }
        self.registry.lock().unwrap().insert(descriptor);
        Ok(())
    }

    /// `runtime.estimate` — docs/22 §Interfaces: candidate `ResourceVector`s
    /// (here, [`ResourceEstimate`]) for every variant that fits and meets
    /// the latency budget, best-first, consumed by
    /// `hyperion-model-router`'s feasibility gate.
    pub fn estimate(
        &self,
        class: ModelClass,
        contract: &CapabilityContract,
    ) -> Vec<ResourceEstimate> {
        let registry = self.registry.lock().unwrap();
        let residency = self.residency.lock().unwrap();
        let available = residency.capacity_available(self.total_capacity_mb);

        registry
            .by_class(class)
            .flat_map(|d| d.variants.iter().copied())
            .filter(|v| v.footprint_mb <= available.max(self.total_capacity_mb))
            .filter(|v| Self::meets_latency_budget(v, contract))
            .map(|v| ResourceEstimate {
                precision: v.precision,
                footprint_mb: v.footprint_mb,
                expected_tokens_per_sec: v.expected_tokens_per_sec,
            })
            .collect()
    }

    fn meets_latency_budget(variant: &QuantizedVariant, contract: &CapabilityContract) -> bool {
        // A rough proxy: assume a ~100-token response and require the
        // variant's throughput to deliver it inside the budget.
        let estimated_ms = (100.0 / variant.expected_tokens_per_sec.max(0.01)) * 1000.0;
        estimated_ms <= contract.latency_budget_ms as f32
    }

    /// docs/22 §5.1's hardware-adaptive tier selection: best variant first,
    /// stepping down (lower precision / smaller footprint) on a failed fit
    /// or latency check, per [`ModelDescriptor::variants`]' declared
    /// best-first order. Returns `None` if nothing fits at all — "signal
    /// infeasible to 23/21," never a silent failure.
    ///
    /// This checks only whether a variant could *ever* fit this device's
    /// total capacity and meets the latency budget — whether it fits
    /// *right now*, given current occupancy, is [`Self::load`]'s job (it
    /// may need to evict lower-value residents first, or fail with
    /// [`RuntimeError::InfeasibleLocally`] if even that isn't enough).
    pub fn select_variant(
        &self,
        class: ModelClass,
        contract: &CapabilityContract,
    ) -> Option<(u64, QuantizedVariant)> {
        let registry = self.registry.lock().unwrap();
        for descriptor in registry.by_class(class) {
            for variant in &descriptor.variants {
                if variant.footprint_mb > self.total_capacity_mb {
                    continue;
                }
                if !Self::meets_latency_budget(variant, contract) {
                    continue;
                }
                return Some((descriptor.model_id, *variant));
            }
        }
        None
    }

    /// `runtime.load` — ensures the model is `Hot`, evicting lower-value
    /// residents if needed (docs/22 §5.2).
    pub fn load(&self, model_id: u64, variant: &QuantizedVariant) -> Result<(), RuntimeError> {
        let mut residency = self.residency.lock().unwrap();
        if residency.is_hot(model_id) {
            residency.touch(model_id, now());
            return Ok(());
        }
        if !residency.make_room(variant.footprint_mb, self.total_capacity_mb, now()) {
            return Err(RuntimeError::InfeasibleLocally);
        }
        residency.mark_hot(model_id, variant.footprint_mb, now());
        Ok(())
    }

    /// `runtime.infer` — capability-gated (docs/22 §Security
    /// Considerations' Trust-Boundary-scoped Execution Engine, simplified
    /// here to a single rights check per invocation rather than a real
    /// per-boundary KV-cache/batching model). Resolves a variant, loads it
    /// if needed (downgrading first under a constrained
    /// [`PowerMode`], per §5.3), and runs the mock backend synchronously.
    pub fn infer(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        class: ModelClass,
        contract: &CapabilityContract,
        request: &InferenceRequest,
    ) -> Result<InferenceResult, RuntimeError> {
        monitor
            .check_rights_ok_result(token, RightsMask::EXEC)
            .map_err(|_| RuntimeError::Unauthorized)?;

        let power = *self.power_mode.lock().unwrap();
        let (model_id, variant) = match (self.select_variant(class, contract), power) {
            (Some((id, v)), PowerMode::BatterySaver | PowerMode::Critical) => {
                (id, self.downgrade(class, v))
            }
            (Some((id, v)), _) => (id, v),
            (None, _) => return Err(RuntimeError::InfeasibleLocally),
        };

        self.load(model_id, &variant)?;
        let text = self.backend.generate(model_id, request);
        self.residency.lock().unwrap().touch(model_id, now());
        let _request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);

        Ok(InferenceResult {
            text,
            tokens_generated: 0,
            variant_used: variant.precision,
        })
    }

    /// §5.3: under a constrained power budget, prefer the smallest
    /// resident-eligible variant of the same class rather than the one
    /// [`Self::select_variant`] would otherwise pick.
    fn downgrade(&self, class: ModelClass, current: QuantizedVariant) -> QuantizedVariant {
        let registry = self.registry.lock().unwrap();
        registry
            .by_class(class)
            .flat_map(|d| d.variants.iter().copied())
            .filter(|v| v.precision <= current.precision)
            .min_by_key(|v| v.footprint_mb)
            .unwrap_or(current)
    }

    /// A no-op stub — see this crate's doc comment on deferred streaming.
    pub fn cancel(&self, _request_id: u64) {}

    pub fn residency_of(&self, model_id: u64) -> Option<ResidencyEntry> {
        self.residency.lock().unwrap().entry(model_id)
    }

    pub fn descriptor(&self, model_id: u64) -> Option<ModelDescriptor> {
        self.registry.lock().unwrap().get(model_id).cloned()
    }

    pub fn pin(&self, model_id: u64) {
        self.residency.lock().unwrap().pin(model_id);
    }

    pub fn unpin(&self, model_id: u64) {
        self.residency.lock().unwrap().unpin(model_id);
    }

    pub fn set_predicted_next_use(&self, model_id: u64, value: f32) {
        self.residency
            .lock()
            .unwrap()
            .set_predicted_next_use(model_id, value);
    }

    pub fn set_power_mode(&self, mode: PowerMode) {
        *self.power_mode.lock().unwrap() = mode;
    }

    pub fn power_mode(&self) -> PowerMode {
        *self.power_mode.lock().unwrap()
    }
}
