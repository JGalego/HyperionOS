use std::collections::HashMap;

use crate::types::{ImplId, ImplementationDescriptor, RolloutStage};

/// docs/23 §Recovery Mechanisms: "each `ImplementationDescriptor` carries a
/// circuit breaker: after n consecutive failures it is temporarily demoted
/// to the bottom of any fallback chain... until a cooldown and a
/// successful health probe restore it." This crate's cooldown is simply
/// "the next successful `report_outcome`" — there is no real health-probe
/// mechanism yet (nothing to probe; no real dispatch exists).
const CIRCUIT_BREAKER_THRESHOLD: u32 = 3;

#[derive(Debug, Default)]
pub(crate) struct CircuitBreaker {
    consecutive_failures: HashMap<ImplId, u32>,
}

impl CircuitBreaker {
    pub(crate) fn record_success(&mut self, impl_id: ImplId) {
        self.consecutive_failures.remove(&impl_id);
    }

    pub(crate) fn record_failure(&mut self, impl_id: ImplId) {
        *self.consecutive_failures.entry(impl_id).or_insert(0) += 1;
    }

    pub(crate) fn is_open(&self, impl_id: ImplId) -> bool {
        self.consecutive_failures
            .get(&impl_id)
            .copied()
            .unwrap_or(0)
            >= CIRCUIT_BREAKER_THRESHOLD
    }
}

/// docs/23 §Architecture's Candidate Gathering, standing in for a real
/// Capability Registry ([24 — Plugin Framework](../24-plugin-framework.md)) —
/// see this crate's doc comment.
#[derive(Debug, Default)]
pub struct ImplementationRegistry {
    descriptors: HashMap<ImplId, ImplementationDescriptor>,
}

impl ImplementationRegistry {
    /// docs/23 §Architecture: "new `ImplementationDescriptor`s enter at
    /// `Shadow`" — enforced here regardless of what the caller passed in,
    /// since promotion is meant to be a deliberate, separate decision
    /// ([`crate::ModelRouter::set_rollout_stage`]), never implicit at
    /// registration.
    pub(crate) fn register(&mut self, mut descriptor: ImplementationDescriptor) {
        descriptor.rollout_stage = RolloutStage::Shadow;
        self.descriptors.insert(descriptor.impl_id, descriptor);
    }

    pub(crate) fn set_rollout_stage(&mut self, impl_id: ImplId, stage: RolloutStage) {
        if let Some(descriptor) = self.descriptors.get_mut(&impl_id) {
            descriptor.rollout_stage = stage;
        }
    }

    pub(crate) fn by_capability<'a>(
        &'a self,
        capability_id: &'a str,
    ) -> impl Iterator<Item = &'a ImplementationDescriptor> {
        self.descriptors
            .values()
            .filter(move |d| d.capability_id == capability_id)
    }

    pub(crate) fn get(&self, impl_id: ImplId) -> Option<&ImplementationDescriptor> {
        self.descriptors.get(&impl_id)
    }
}
