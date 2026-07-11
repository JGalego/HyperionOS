/// docs/22 §Data Structures' `ModelClass` — functional family; which one
/// applies is decided by [23 — Multi-Model Orchestration](../23-multi-model-orchestration.md),
/// not this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelClass {
    Slm,
    Lrm,
    Vision,
    Speech,
    Coding,
    Planning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Precision {
    Int4,
    Int8,
    Fp16,
}

/// docs/22 §Data Structures' `QuantizedVariant`, narrowed to a single
/// simulated hardware profile (`expected_tokens_per_sec` is a scalar, not a
/// `Map<HardwareProfile, f32>`) — a hosted simulator has exactly one device.
#[derive(Debug, Clone, Copy)]
pub struct QuantizedVariant {
    pub precision: Precision,
    pub footprint_mb: u32,
    pub expected_tokens_per_sec: f32,
}

/// docs/22 §Data Structures' `ModelDescriptor`. `signature` (PRODUCTION_BOOT_PROMPT.md M9) is a
/// real Ed25519 signature over [`crate::sign`]'s canonical bytes — see this crate's doc comment.
/// `None` until a caller signs it via [`crate::sign`]; [`crate::runtime::LocalAiRuntime::register_model`]
/// always rejects a `None` or invalid signature, never treats "unsigned" as "trust it anyway."
#[derive(Debug, Clone)]
pub struct ModelDescriptor {
    pub model_id: u64,
    pub class: ModelClass,
    /// Best-quality first — [`crate::runtime::LocalAiRuntime::select_variant`]
    /// walks this in order per docs/22 §5.1.
    pub variants: Vec<QuantizedVariant>,
    pub signature: Option<hyperion_crypto::Signature>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidencyStatus {
    Hot,
    Warm,
    Cold,
}

/// docs/22 §Data Structures' `ResidencyEntry`.
#[derive(Debug, Clone, Copy)]
pub struct ResidencyEntry {
    pub model_id: u64,
    pub status: ResidencyStatus,
    pub last_used: u64,
    pub pin_count: u32,
    /// docs/22 §5.2: "read, not computed independently — it is
    /// [06 — Context Engine]'s working-set signal." This crate accepts it
    /// as a caller-supplied value (via
    /// [`crate::runtime::LocalAiRuntime::set_predicted_next_use`]) rather
    /// than depending on `hyperion-context` directly, since residency
    /// management should not need to know *why* a model is likely to be
    /// used again.
    pub predicted_next_use: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerMode {
    Performance,
    Balanced,
    BatterySaver,
    Critical,
}

/// docs/22 §Data Structures' `InferenceHandle`/request pair, collapsed into
/// one synchronous call — see this crate's doc comment on deferred
/// streaming.
#[derive(Debug, Clone)]
pub struct InferenceRequest {
    pub prompt: String,
}

#[derive(Debug, Clone)]
pub struct InferenceResult {
    pub text: String,
    pub tokens_generated: u32,
    pub variant_used: Precision,
}

/// A narrowed stand-in for docs/22 §Scope Boundary's `capability_contract`
/// argument to `runtime.estimate` — just enough for the fit/latency check
/// in §5.1, not the full Capability semantic contract (which does not exist
/// as a concrete type until later phases' Capability/Plugin work).
#[derive(Debug, Clone, Copy)]
pub struct CapabilityContract {
    pub latency_budget_ms: u64,
    pub always_on: bool,
}

/// One feasible candidate returned by `runtime.estimate` — docs/22
/// §Interfaces' `[CandidateResourceVector]`, narrowed to the fields this
/// crate actually has an opinion about (no `battery_mw`/`context_window_slots`
/// dimensions yet, since there is no real `hyperion-scheduler` integration —
/// see this crate's doc comment).
#[derive(Debug, Clone, Copy)]
pub struct ResourceEstimate {
    pub precision: Precision,
    pub footprint_mb: u32,
    pub expected_tokens_per_sec: f32,
}
