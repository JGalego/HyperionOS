use serde::{Deserialize, Serialize};

/// docs/11 §5.1's `TrustTier`, 1:1 with docs/15's `provenance_tier` —
/// declaration order gives the derived `Ord` docs/11 §7's pseudocode
/// depends on: `System(0) < Verified(1) < Community(2)`, so a *lower*
/// value is *more* trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TrustTier {
    System,
    Verified,
    Community,
}

/// docs/11 §5.1's `AgentManifest`, narrowed per this crate's doc comment
/// (no `sandbox_class` — see there). `checksum` stands in for a real
/// signature, the same non-cryptographic pattern used throughout this
/// workspace (`hyperion-context`'s envelope integrity,
/// `hyperion-ai-runtime`'s model registration).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentManifest {
    pub specialization: String,
    pub baseline_capabilities: Vec<String>,
    pub requestable_capabilities: Vec<String>,
    pub trust_tier: TrustTier,
}

/// docs/11 §3.3's lifecycle state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleState {
    Spawning,
    Bound,
    Executing,
    WaitingOnCapability,
    Suspended,
    Checkpointed,
    Completed,
    Failed,
    Terminated,
}

/// docs/11 §5.3's `CapabilityGrant`, narrowed to what this crate's Broker
/// actually checks (no `revocation_hook` — checkpoint/resume revokes by
/// simply dropping the grants list, per §6.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityGrant {
    pub capability_ref: String,
    pub scope: Vec<u64>,
    pub granted_at: u64,
}

/// docs/11 §6.2's token-bucket quota, narrowed to the single
/// Capability-calls-per-window dimension and the consecutive-failure
/// circuit breaker — see this crate's doc comment on deferred Scheduler
/// integration for the CPU/GPU/token dimensions.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct QuotaState {
    pub calls_used_this_window: u32,
    pub max_calls_per_window: u32,
    pub consecutive_failures: u32,
    /// Real epoch-seconds ([`crate::runtime::now`]) this instance was last suspended --
    /// informational display/audit metadata only. `None` once resumed. The actual adaptive
    /// backoff window (keyed on [`Self::times_suspended`]) is gated by
    /// [`crate::AgentRuntime::prepare_invoke`] against a real monotonic clock kept separately
    /// (`AgentRuntime::suspended_since`), not this field -- whole-second epoch arithmetic can
    /// make a window appear to have already elapsed after up to just under one second less real
    /// wait than it actually requires.
    pub suspended_at: Option<u64>,
    /// docs/998-roadmap.md's Self-Sustaining pillar: this instance's *whole-life* suspension
    /// count, distinct from [`Self::consecutive_failures`] (which already resets to `0` on any
    /// success -- the real "is this instance still in trouble right now" signal). This one never
    /// resets on a single success; it's the real "has this instance made the same kind of mistake
    /// before" history a repeat offender's own, longer backoff is computed from, and a real streak
    /// of successes after a resume decays it back down (see
    /// [`crate::AgentRuntime::record_success_after_resume`]) -- the actual "recovers, and comes
    /// out stronger" mechanic, not a fixed penalty forever.
    pub times_suspended: u32,
    /// How many *consecutive* real successes this instance has had since its last resume --
    /// counted only while [`Self::times_suspended`] is still above zero, and reset by any real
    /// failure. Once this reaches [`crate::runtime::SUCCESS_STREAK_TO_DECAY`], `times_suspended`
    /// decays by one and this counter resets -- the concrete trigger for "earns its caution back."
    pub consecutive_successes_since_resume: u32,
}

impl QuotaState {
    pub fn new(max_calls_per_window: u32) -> Self {
        QuotaState {
            calls_used_this_window: 0,
            max_calls_per_window,
            consecutive_failures: 0,
            suspended_at: None,
            times_suspended: 0,
            consecutive_successes_since_resume: 0,
        }
    }

    pub fn has_headroom(&self) -> bool {
        self.calls_used_this_window < self.max_calls_per_window
    }
}

/// docs/11 §5.2's `AgentInstance`, narrowed per this crate's doc comment
/// (no `context_bundle`/`parent_session` binding yet — `bound_intent` is
/// the one reference this slice binds).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInstance {
    pub instance_id: u64,
    pub manifest: AgentManifest,
    pub state: LifecycleState,
    pub bound_intent: Option<u64>,
    pub grants: Vec<CapabilityGrant>,
    pub quota: QuotaState,
    /// Set while `state == WaitingOnCapability` — the capability awaiting
    /// [`crate::AgentRuntime::resolve_consent`].
    pub pending_consent: Option<String>,
    pub audit_log: Vec<AuditEntry>,
}

/// docs/11 §5.3's `AgentCheckpoint`, narrowed to the manifest and bound
/// Intent reference — see this crate's doc comment on deferred serialized
/// reasoning state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCheckpoint {
    pub checkpoint_id: u64,
    pub instance_id: u64,
    pub manifest: AgentManifest,
    pub bound_intent: Option<u64>,
    pub created_at: u64,
}

/// One entry in docs/11 §5.4's Agent Execution Record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: u64,
    pub kind: String,
    pub detail: String,
}

/// The result of [`crate::AgentRuntime::invoke`] — docs/11 §8's pseudocode
/// branches (`grant is PENDING_CONSENT`, `grant is DENIED`, quota
/// exhaustion, a dispatched result) made into a typed return value instead
/// of a control-flow loop, since this crate has no real reasoning loop to
/// drive that control flow.
#[derive(Debug, Clone)]
pub enum InvokeOutcome {
    Result(serde_json::Value),
    Denied,
    PendingConsent,
    QuotaExceeded,
    /// The dispatched stub capability itself reported failure — see this
    /// crate's doc comment on `{"force_fail": true}`.
    Failed(String),
}
