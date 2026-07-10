use std::time::Instant;

use hyperion_capability::CapabilityToken;

/// One resource request or allocation, spanning classical AND AI-specific
/// dimensions — docs/04-scheduler.md §Data Structures. Every task is
/// described in this vector, never in CPU-only terms.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ResourceVector {
    pub cpu_shares: u32,
    pub ram_mb: u32,
    pub gpu_shares: u32,
    pub vram_mb: u32,
    pub storage_iops: u32,
    pub network_bw_kbps: u32,
    pub inference_tokens_per_sec: u32,
    pub context_window_slots: u32,
    pub battery_budget_mw: u32,
}

/// The nine dimensions a [`ResourceVector`] spans, as an enumerable set so
/// admission control and DRF ranking can iterate "every dimension a task
/// touches" generically instead of hand-unrolling nine field accesses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceDimension {
    Cpu,
    Ram,
    Gpu,
    Vram,
    StorageIops,
    NetworkBw,
    InferenceTokens,
    ContextWindowSlots,
    Battery,
}

impl ResourceDimension {
    pub const ALL: [ResourceDimension; 9] = [
        ResourceDimension::Cpu,
        ResourceDimension::Ram,
        ResourceDimension::Gpu,
        ResourceDimension::Vram,
        ResourceDimension::StorageIops,
        ResourceDimension::NetworkBw,
        ResourceDimension::InferenceTokens,
        ResourceDimension::ContextWindowSlots,
        ResourceDimension::Battery,
    ];
}

impl ResourceVector {
    pub fn get(&self, dim: ResourceDimension) -> u32 {
        match dim {
            ResourceDimension::Cpu => self.cpu_shares,
            ResourceDimension::Ram => self.ram_mb,
            ResourceDimension::Gpu => self.gpu_shares,
            ResourceDimension::Vram => self.vram_mb,
            ResourceDimension::StorageIops => self.storage_iops,
            ResourceDimension::NetworkBw => self.network_bw_kbps,
            ResourceDimension::InferenceTokens => self.inference_tokens_per_sec,
            ResourceDimension::ContextWindowSlots => self.context_window_slots,
            ResourceDimension::Battery => self.battery_budget_mw,
        }
    }

    pub fn set(&mut self, dim: ResourceDimension, value: u32) {
        match dim {
            ResourceDimension::Cpu => self.cpu_shares = value,
            ResourceDimension::Ram => self.ram_mb = value,
            ResourceDimension::Gpu => self.gpu_shares = value,
            ResourceDimension::Vram => self.vram_mb = value,
            ResourceDimension::StorageIops => self.storage_iops = value,
            ResourceDimension::NetworkBw => self.network_bw_kbps = value,
            ResourceDimension::InferenceTokens => self.inference_tokens_per_sec = value,
            ResourceDimension::ContextWindowSlots => self.context_window_slots = value,
            ResourceDimension::Battery => self.battery_budget_mw = value,
        }
    }

    pub fn iter_dimensions(&self) -> impl Iterator<Item = (ResourceDimension, u32)> + '_ {
        ResourceDimension::ALL.into_iter().map(|d| (d, self.get(d)))
    }

    pub fn saturating_add_assign(&mut self, other: &ResourceVector) {
        for (d, v) in other.iter_dimensions() {
            self.set(d, self.get(d).saturating_add(v));
        }
    }

    pub fn saturating_sub_assign(&mut self, other: &ResourceVector) {
        for (d, v) in other.iter_dimensions() {
            self.set(d, self.get(d).saturating_sub(v));
        }
    }
}

/// docs/04-scheduler.md §Data Structures. `RealTimeUI` is dispatched by
/// Earliest-Deadline-First against reserved headroom; the other three share
/// remaining capacity via Dominant Resource Fairness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedClass {
    RealTimeUI,
    InteractiveAgent,
    BackgroundAgent,
    BatchDistributable,
}

/// Placeholder identifiers standing in for docs/05-intent-engine.md's
/// `IntentId` and docs/11-agent-runtime.md's `AgentId`, neither of which
/// exists yet (both are Phase 3/4 subsystems). The scheduler only needs
/// *an* opaque, hashable owner identity to key `OwnerAccount` by — these
/// newtypes are replaced by the real Intent Engine / Agent Runtime types
/// when those phases land, without changing anything in this crate's
/// fairness algorithm, which only depends on [`OwnerId`] being hashable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IntentId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AgentId(pub u64);

/// The unit DRF fairness is computed per — an Agent if one owns the task,
/// otherwise the bare Intent that submitted it directly
/// (docs/04-scheduler.md §Data Structures).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OwnerId {
    Agent(AgentId),
    Intent(IntentId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(pub u64);

/// Handle returned by `submit_task`, per docs/04-scheduler.md §Interfaces /
/// APIs. Always equal to the submitted [`TaskDescriptor`]'s own `id` in
/// this simulator — callers mint their own unique `TaskId`s rather than the
/// scheduler assigning one, since nothing else here needs to be the source
/// of task identity.
pub type Ticket = TaskId;

#[derive(Debug, Clone)]
pub struct TaskDescriptor {
    pub id: TaskId,
    pub owner_intent: IntentId,
    pub owner_agent: Option<AgentId>,
    pub class: SchedClass,
    /// Required for `RealTimeUI`; an optional hint otherwise.
    pub deadline: Option<Instant>,
    pub priority_weight: f32,
    pub request: ResourceVector,
    /// Per docs/03-kernel-architecture.md; checked by `submit_task` before
    /// the task is ever queued, per docs/04 §Security Considerations.
    pub cap_token: CapabilityToken,
}

pub(crate) fn owner_of(task: &TaskDescriptor) -> OwnerId {
    task.owner_agent
        .map(OwnerId::Agent)
        .unwrap_or(OwnerId::Intent(task.owner_intent))
}
