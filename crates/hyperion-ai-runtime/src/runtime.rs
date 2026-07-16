use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
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

/// docs/22's own previously-named "cancellable streaming" gap, closed for real generation loops
/// that actually have a per-step boundary to check at: a real, shareable cancellation signal one
/// thread can flip via [`LocalAiRuntime::cancel`] while another thread's real, in-progress
/// [`LocalAiRuntime::infer_cancellable`] call (running [`InferenceBackend::generate`] on it) is
/// still running. Honest scope: only a backend with a genuine token-by-token (or otherwise
/// interruptible) loop can act on this before its own `generate` call returns —
/// [`crate::candle_backend::CandleBackend`] is the one real backend in this workspace with such a
/// loop; every HTTP-backed backend (`OpenAiCompatBackend`/`AnthropicBackend`/`GeminiBackend`) and
/// [`crate::MockBackend`] still complete their one blocking call before anything could check this,
/// so they receive it and simply never consult it -- named here, not silently pretended otherwise.
#[derive(Clone)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    fn new() -> Self {
        CancellationToken {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// A real, permanently-uncancelled token for a caller (e.g. [`LocalAiRuntime::infer`]'s own
    /// existing, unchanged entry point) that has no real cancellation id to check against.
    pub fn never_cancelled() -> Self {
        Self::new()
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

/// A pluggable execution backend — see this crate's doc comment. Swapping
/// [`crate::MockBackend`] for a real one is the entire integration surface
/// a future session needs to touch to run real models. `cancel` is real, checkable state (see
/// [`CancellationToken`]'s own doc comment on which real backends can and can't act on it).
pub trait InferenceBackend: Send + Sync {
    fn generate(
        &self,
        model_id: u64,
        request: &InferenceRequest,
        cancel: &CancellationToken,
    ) -> String;
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
    /// `Arc`, not a bare `Box`, specifically so [`Self::infer`] can clone the *currently* active
    /// backend out from behind this lock and call its real (potentially slow -- a real network
    /// round trip to a real cloud model) `generate` with no lock held at all. A real,
    /// previously-shipped bottleneck this fixes: holding this lock across `generate` itself would
    /// serialize every concurrent `infer` call in the whole runtime behind it, no matter how many
    /// real OS threads a caller spawned to dispatch independent work -- the same class of bug
    /// [`hyperion_agent_runtime::AgentRuntime::invoke`]'s own three-phase split fixes one layer up.
    backend: Mutex<Arc<dyn InferenceBackend>>,
    total_capacity_mb: u32,
    next_request_id: AtomicU64,
    /// Real, in-progress [`Self::infer_cancellable`] calls, keyed by their own real,
    /// caller-supplied `request_id` -- what [`Self::cancel`] actually flips. An entry only ever
    /// exists for the duration of one real `generate` call; removed the instant it returns, so a
    /// `cancel` for an id that already finished (or was never registered, e.g. a caller of the
    /// plain [`Self::infer`]) is a real, harmless no-op, never a dangling reference.
    in_flight: Mutex<HashMap<u64, CancellationToken>>,
}

impl LocalAiRuntime {
    pub fn new(backend: Box<dyn InferenceBackend>, total_capacity_mb: u32) -> Self {
        LocalAiRuntime {
            registry: Mutex::new(ModelRegistry::default()),
            residency: Mutex::new(ResidencyManager::default()),
            power_mode: Mutex::new(PowerMode::Performance),
            backend: Mutex::new(Arc::from(backend)),
            total_capacity_mb,
            next_request_id: AtomicU64::new(1),
            in_flight: Mutex::new(HashMap::new()),
        }
    }

    /// Swaps the active backend in place -- the console's `/backend`/`use backend` meta-command
    /// needs this to move between `MockBackend` and a real one without restarting the process.
    /// Takes effect starting with the very next [`Self::infer`] call; registered models,
    /// residency, and power mode are untouched.
    pub fn set_backend(&self, backend: Box<dyn InferenceBackend>) {
        *self.backend.lock().unwrap() = Arc::from(backend);
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
    /// Never cancellable through this entry point -- no `request_id` is ever surfaced to a
    /// caller to cancel it with. See [`Self::infer_cancellable`] for the real, cancellable path.
    pub fn infer(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        class: ModelClass,
        contract: &CapabilityContract,
        request: &InferenceRequest,
    ) -> Result<InferenceResult, RuntimeError> {
        self.infer_with_token(
            monitor,
            token,
            class,
            contract,
            request,
            &CancellationToken::never_cancelled(),
        )
    }

    /// As [`Self::infer`], but real caller-supplied `request_id` is registered against a real
    /// [`CancellationToken`] *before* the real (potentially slow) `InferenceBackend::generate`
    /// call runs, so a genuinely concurrent caller on another real thread can really cancel it
    /// mid-generation via [`Self::cancel`] -- closes docs/22's own previously-named "cancellable
    /// streaming" gap for real, for whichever real backend can act on it (see
    /// [`CancellationToken`]'s own doc comment for the honest boundary on which ones can).
    /// `request_id` is the caller's own to choose (and to remember, to cancel with later) --
    /// this crate mints no id of its own for this path, unlike [`Self::infer`]'s internal,
    /// never-surfaced one.
    pub fn infer_cancellable(
        &self,
        request_id: u64,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        class: ModelClass,
        contract: &CapabilityContract,
        request: &InferenceRequest,
    ) -> Result<InferenceResult, RuntimeError> {
        let cancel = CancellationToken::new();
        self.in_flight
            .lock()
            .unwrap()
            .insert(request_id, cancel.clone());
        let result = self.infer_with_token(monitor, token, class, contract, request, &cancel);
        self.in_flight.lock().unwrap().remove(&request_id);
        result
    }

    fn infer_with_token(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        class: ModelClass,
        contract: &CapabilityContract,
        request: &InferenceRequest,
        cancel: &CancellationToken,
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
        // Clone the `Arc` (cheap: a refcount bump, not a copy of the backend itself) and drop
        // the lock immediately -- `generate` below runs with no lock held, so it can be a real,
        // slow network call without serializing any other concurrent `infer` call. See this
        // struct's own `backend` field doc comment.
        let backend = self.backend.lock().unwrap().clone();
        let text = backend.generate(model_id, request, cancel);
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

    /// Real now: flips the real [`CancellationToken`] registered for `request_id` (via
    /// [`Self::infer_cancellable`]), if that request is still in flight. A harmless no-op for an
    /// id that already finished, was never registered (e.g. a plain [`Self::infer`] call), or
    /// belongs to a backend with no real per-step boundary to check it at.
    pub fn cancel(&self, request_id: u64) {
        if let Some(token) = self.in_flight.lock().unwrap().get(&request_id) {
            token.cancel();
        }
    }

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
