use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use hyperion_ai_runtime::{CapabilityContract, LocalAiRuntime};
use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};

use crate::registry::{CircuitBreaker, ImplementationRegistry};
use crate::types::{
    CapabilityInvocation, ConsequenceTier, CostModel, ExclusionReason, ImplId, ImplKind,
    ImplementationDescriptor, PrivacyTier, Rationale, ResourceCost, RolloutStage, RoutingDecision,
    RoutingScore, UrgencyClass,
};

#[derive(Debug, thiserror::Error)]
pub enum ModelRouterError {
    #[error("capability does not authorize registering or promoting a Model Router candidate")]
    Unauthorized,
}

struct WeightVector {
    lat: f32,
    priv_: f32,
    cost: f32,
    qual: f32,
    avail: f32,
}

impl WeightVector {
    /// docs/23 §Algorithms 3: base weights by `urgency_class`, then a
    /// `HighStakes` floor on `w_qual` — renormalized so the five weights
    /// still sum to 1.0, keeping `composite()` inspectable as a true
    /// weighted average per [18 — Explainability & Trust](../18-explainability-and-trust.md).
    fn for_invocation(urgency: UrgencyClass, consequence: ConsequenceTier) -> WeightVector {
        let mut w = match urgency {
            UrgencyClass::Interactive => WeightVector {
                lat: 0.40,
                priv_: 0.15,
                cost: 0.10,
                qual: 0.25,
                avail: 0.10,
            },
            UrgencyClass::Background => WeightVector {
                lat: 0.15,
                priv_: 0.20,
                cost: 0.20,
                qual: 0.30,
                avail: 0.15,
            },
            UrgencyClass::Batch => WeightVector {
                lat: 0.05,
                priv_: 0.15,
                cost: 0.30,
                qual: 0.35,
                avail: 0.15,
            },
        };

        if consequence == ConsequenceTier::HighStakes && w.qual < 0.5 {
            let deficit = 0.5 - w.qual;
            let others = w.lat + w.priv_ + w.cost + w.avail;
            let shrink = if others > 0.0 {
                1.0 - deficit / others
            } else {
                1.0
            };
            w.lat *= shrink;
            w.priv_ *= shrink;
            w.cost *= shrink;
            w.avail *= shrink;
            w.qual = 0.5;
        }
        w
    }
}

/// docs/23 — Multi-Model Orchestration's Model Router, scoped to this
/// phase's "single-model routing scaffold" per docs/41. See this crate's
/// doc comment for what's deferred.
pub struct ModelRouter {
    registry: Mutex<ImplementationRegistry>,
    circuit_breaker: Mutex<CircuitBreaker>,
    ai_runtime: std::sync::Arc<LocalAiRuntime>,
    next_invocation_id: AtomicU64,
}

impl ModelRouter {
    pub fn new(ai_runtime: std::sync::Arc<LocalAiRuntime>) -> Self {
        ModelRouter {
            registry: Mutex::new(ImplementationRegistry::default()),
            circuit_breaker: Mutex::new(CircuitBreaker::default()),
            ai_runtime,
            next_invocation_id: AtomicU64::new(1),
        }
    }

    fn require(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        rights: RightsMask,
    ) -> Result<(), ModelRouterError> {
        monitor
            .check_rights_ok_result(token, rights)
            .map_err(|_| ModelRouterError::Unauthorized)
    }

    /// docs/23 §Interfaces' `register_implementation` — always enters at
    /// `Shadow` regardless of the descriptor's own field, per §Architecture.
    /// Capability-gated: this crate's own doc comment named registration
    /// as "not capability-gated here," since the Plugin Framework was
    /// meant to be the real Trust Boundary check an "install/register an
    /// implementation" crossing goes through. `hyperion-plugin-framework`
    /// itself already re-checks its own installer's rights before this is
    /// ever reached (via `hyperion-api-gateway`'s bridge), so this is a
    /// second, independent gate — the same "every layer re-checks live,
    /// never trusts a caller's prior check" convention this workspace
    /// already uses everywhere else.
    pub fn register_implementation(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        descriptor: ImplementationDescriptor,
    ) -> Result<(), ModelRouterError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        self.registry.lock().unwrap().register(descriptor);
        Ok(())
    }

    /// docs/23 §Interfaces' `set_rollout_stage` — called by
    /// [32 — Update System](../32-update-system.md) in the real system;
    /// exposed directly here since that document doesn't exist yet.
    /// Capability-gated for the same reason as [`Self::register_implementation`].
    pub fn set_rollout_stage(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        impl_id: ImplId,
        stage: RolloutStage,
    ) -> Result<(), ModelRouterError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        self.registry
            .lock()
            .unwrap()
            .set_rollout_stage(impl_id, stage);
        Ok(())
    }

    pub fn descriptor(&self, impl_id: ImplId) -> Option<ImplementationDescriptor> {
        self.registry.lock().unwrap().get(impl_id).cloned()
    }

    /// docs/23 §Interfaces' `report_outcome` — feeds the circuit breaker
    /// (§Recovery Mechanisms).
    pub fn report_outcome(&self, impl_id: ImplId, success: bool) {
        let mut breaker = self.circuit_breaker.lock().unwrap();
        if success {
            breaker.record_success(impl_id);
        } else {
            breaker.record_failure(impl_id);
        }
    }

    /// `hyperion-scheduler`'s own named "model-tier degradation" gap, made real: every real,
    /// non-`Shadow`, not-circuit-broken registered implementation for `capability_id` that
    /// declares a resource cost. `Scheduler::schedule_epoch`'s non-admit branch is the real
    /// caller — it reads this looking for a cheaper tier to retry admission with instead of
    /// just aging and requeuing the original request forever. Deliberately not `route()`'s own
    /// scoring pipeline: degradation needs "what would fit," not "what scores best," and has no
    /// `CapabilityInvocation` (urgency/consequence/consent) to score against in the first place.
    pub fn declared_costs(&self, capability_id: &str) -> Vec<(ImplId, ResourceCost)> {
        let breaker = self.circuit_breaker.lock().unwrap();
        self.registry
            .lock()
            .unwrap()
            .by_capability(capability_id)
            .filter(|d| d.rollout_stage != RolloutStage::Shadow)
            .filter(|d| !breaker.is_open(d.impl_id))
            .filter_map(|d| d.resource_cost.map(|cost| (d.impl_id, cost)))
            .collect()
    }

    fn latency_fit(&self, descriptor: &ImplementationDescriptor, budget_ms: u64) -> f32 {
        let estimated_ms = match (descriptor.kind, descriptor.model_class) {
            (ImplKind::LocalSmallModel | ImplKind::LocalLargeModel, Some(class)) => {
                let contract = CapabilityContract {
                    latency_budget_ms: u64::MAX, // don't let estimate() itself filter; we score the fit
                    always_on: false,
                };
                self.ai_runtime
                    .estimate(class, &contract)
                    .into_iter()
                    .map(|e| (100.0 / e.expected_tokens_per_sec.max(0.01)) * 1000.0)
                    .fold(f32::INFINITY, f32::min)
            }
            _ => descriptor.declared_latency_ms as f32,
        };
        if !estimated_ms.is_finite() {
            return 0.0; // no local variant fits at all
        }
        if estimated_ms <= budget_ms as f32 {
            1.0
        } else {
            (budget_ms as f32 / estimated_ms).clamp(0.0, 1.0)
        }
    }

    fn cost_fit(&self, descriptor: &ImplementationDescriptor) -> f32 {
        match descriptor.cost_model {
            CostModel::Free => 1.0,
            CostModel::PerCall(c) => (1.0 / (1.0 + c)) as f32,
            CostModel::PerToken(c) => (1.0 / (1.0 + c * 100.0)) as f32,
        }
    }

    /// Real, deterministic traffic sampling for a `RolloutStage::Canary(pct)` candidate: `true`
    /// exactly when this call's own `(invocation_id, impl_id)` pair hashes into the bottom `pct`
    /// fraction of the real `u64` hash space — over many real, distinct `invocation_id`s this
    /// converges to genuinely routing `pct` of live traffic to this candidate, per docs/23's own
    /// "a small percentage of live invocations" framing. Deterministic (not a real RNG) so the
    /// same `invocation_id` always reproduces the same real sampling decision — useful for
    /// replaying/debugging a specific routing decision — and independent per `impl_id`, so two
    /// different Canary candidates for the same capability sample independently rather than
    /// moving together.
    fn canary_sampled_in(invocation_id: u64, impl_id: ImplId, pct: f32) -> bool {
        if pct <= 0.0 {
            return false;
        }
        if pct >= 1.0 {
            return true;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        invocation_id.hash(&mut hasher);
        impl_id.hash(&mut hasher);
        let sample = hasher.finish() as f64 / u64::MAX as f64;
        sample < pct as f64
    }

    fn is_locally_feasible(&self, descriptor: &ImplementationDescriptor) -> bool {
        match (descriptor.kind, descriptor.model_class) {
            (ImplKind::LocalSmallModel | ImplKind::LocalLargeModel, Some(class)) => {
                let contract = CapabilityContract {
                    latency_budget_ms: u64::MAX,
                    always_on: false,
                };
                !self.ai_runtime.estimate(class, &contract).is_empty()
            }
            _ => true, // non-local kinds have no local residency to check
        }
    }

    /// `route` — docs/23 §Pseudocode. Candidate gathering, the privacy gate
    /// (a hard exclusion, never a score), the feasibility gate, weighted
    /// scoring, and the fallback chain. Real percentage-based canary traffic splitting: a
    /// `RolloutStage::Canary(pct)` candidate is a real candidate only for the real fraction `pct`
    /// of calls [`Self::canary_sampled_in`] deterministically samples in, keyed on this call's
    /// own real `invocation_id` — the rest fall straight through to the next-best real candidate
    /// (typically an already-GA implementation), docs/23's own "existing fallback chain still
    /// live as a safety net."
    pub fn route(&self, invocation: &CapabilityInvocation) -> RoutingDecision {
        let invocation_id = self.next_invocation_id.fetch_add(1, Ordering::Relaxed);
        let registry = self.registry.lock().unwrap();
        let breaker = self.circuit_breaker.lock().unwrap();
        let weights =
            WeightVector::for_invocation(invocation.urgency_class, invocation.consequence_tier);

        let mut considered = Vec::new();
        let mut excluded = Vec::new();

        for descriptor in registry.by_capability(&invocation.capability_id) {
            if descriptor.rollout_stage == RolloutStage::Shadow {
                continue; // scored for telemetry only, never chosen — docs/23 §Algorithms 1
            }
            if let RolloutStage::Canary(pct) = descriptor.rollout_stage {
                if !Self::canary_sampled_in(invocation_id, descriptor.impl_id, pct) {
                    excluded.push((descriptor.impl_id, ExclusionReason::CanaryNotSampled));
                    continue;
                }
            }

            // The privacy gate — docs/23 §Architecture: "a gate, not a
            // score component." No weight can rescue a candidate excluded
            // here.
            if descriptor.privacy_tier == PrivacyTier::ConsentedCloud && !invocation.cloud_consent {
                excluded.push((descriptor.impl_id, ExclusionReason::PrivacyGate));
                continue;
            }
            if !self.is_locally_feasible(descriptor) {
                excluded.push((descriptor.impl_id, ExclusionReason::ResourceInfeasible));
                continue;
            }

            let latency_fit = self.latency_fit(descriptor, invocation.latency_budget_ms);
            let privacy_fit = match descriptor.privacy_tier {
                PrivacyTier::Local => 1.0,
                PrivacyTier::ConsentedCloud => 0.6, // still preferred less, even post-gate
            };
            let cost_fit = self.cost_fit(descriptor);
            let quality_fit = descriptor
                .quality_profile
                .get(&invocation.capability_id)
                .copied()
                .unwrap_or(0.5);
            let availability_fit = match descriptor.rollout_stage {
                RolloutStage::Ga => 1.0,
                // Same modest discount this crate always applied to a Canary candidate that
                // survives sampling in — real traffic percentage now genuinely gates *whether*
                // it's even a candidate this call (above); this keeps the "still less proven
                // than GA" scoring nudge for the calls where it is.
                RolloutStage::Canary(_) => 0.8,
                RolloutStage::Shadow => unreachable!("filtered above"),
            };
            // Circuit breaker: demoted to the bottom, never removed —
            // docs/23 §Recovery Mechanisms.
            let availability_fit = if breaker.is_open(descriptor.impl_id) {
                availability_fit * 0.001
            } else {
                availability_fit
            };

            let composite = weights.lat * latency_fit
                + weights.priv_ * privacy_fit
                + weights.cost * cost_fit
                + weights.qual * quality_fit
                + weights.avail * availability_fit;

            considered.push((
                descriptor.impl_id,
                RoutingScore {
                    impl_id: descriptor.impl_id,
                    latency_fit,
                    privacy_fit,
                    cost_fit,
                    quality_fit,
                    availability_fit,
                    composite,
                },
            ));
        }

        considered.sort_by(|a, b| {
            b.1.composite
                .partial_cmp(&a.1.composite)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let needs_verification = invocation.consequence_tier == ConsequenceTier::HighStakes
            || considered.first().is_some_and(|(_, s)| {
                invocation
                    .quality_floor
                    .is_some_and(|floor| s.quality_fit < floor)
            });

        let fallback_chain: Vec<ImplId> = considered.iter().map(|(id, _)| *id).collect();
        let chosen = fallback_chain.first().copied();
        let chosen_reason = match chosen {
            Some(id) => format!(
                "{id:?} scored highest composite among {} feasible candidate(s)",
                considered.len()
            ),
            None => "no candidate survived the privacy/feasibility gates".to_string(),
        };

        RoutingDecision {
            invocation_id,
            chosen,
            fallback_chain,
            rationale: Rationale {
                candidates_considered: considered,
                candidates_excluded: excluded,
                chosen_reason,
                needs_verification,
            },
        }
    }
}
