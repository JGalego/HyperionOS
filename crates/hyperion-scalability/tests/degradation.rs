//! docs/37 §3's `degrade_capability`: fixed fallback order, real consent-
//! gated cloud upgrade, and disable only when nothing else fits.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_privacy::{ConsentLedger, DataScope};
use hyperion_scalability::{
    degrade_capability, CapacityDescriptor, DegradationOutcome, DegradationPolicy, HardwareProfile,
    HardwareTier, ModelTier, ResourceConstraint, Substitution, TenancyMode,
};

fn sbc_profile() -> HardwareProfile {
    HardwareProfile {
        tier: HardwareTier::Sbc,
        compute: CapacityDescriptor {
            ram_mb: 4_096,
            vram_mb: 0,
            compute_tops: 4,
        },
        tenancy: TenancyMode::SingleUser,
    }
}

fn workstation_profile() -> HardwareProfile {
    HardwareProfile {
        tier: HardwareTier::Workstation,
        compute: CapacityDescriptor {
            ram_mb: 65_536,
            vram_mb: 24_576,
            compute_tops: 200,
        },
        tenancy: TenancyMode::SingleUser,
    }
}

fn heavy_vision_policy() -> DegradationPolicy {
    DegradationPolicy {
        capability_ref: "vision.generate".to_string(),
        constraint: ResourceConstraint {
            min_ram_mb: 16_384,
            min_vram_mb: 8_192,
            min_compute_tops: 20,
        },
        fallback_order: vec![
            Substitution::CheaperLocalTier(ModelTier::TinyEdge),
            Substitution::ConsentedCloudUpgrade("acme-cloud".to_string()),
            Substitution::Disable,
        ],
    }
}

#[test]
fn hardware_that_satisfies_the_constraint_runs_at_full_fidelity() {
    let ledger = ConsentLedger::new();
    let plan = degrade_capability(
        Some(&heavy_vision_policy()),
        &workstation_profile(),
        &ledger,
        1,
        |_| false,
        1_000,
    );
    assert_eq!(plan.outcome, DegradationOutcome::FullFidelity);
}

#[test]
fn no_policy_at_all_is_full_fidelity() {
    let ledger = ConsentLedger::new();
    let plan = degrade_capability(None, &sbc_profile(), &ledger, 1, |_| false, 1_000);
    assert_eq!(plan.outcome, DegradationOutcome::FullFidelity);
}

#[test]
fn a_cheaper_local_tier_is_tried_first_when_it_fits() {
    let ledger = ConsentLedger::new();
    let plan = degrade_capability(
        Some(&heavy_vision_policy()),
        &sbc_profile(),
        &ledger,
        1,
        |sub| matches!(sub, Substitution::CheaperLocalTier(_)),
        1_000,
    );
    assert_eq!(
        plan.outcome,
        DegradationOutcome::Substituted {
            substitution: Substitution::CheaperLocalTier(ModelTier::TinyEdge)
        }
    );
}

#[test]
fn without_a_fitting_local_tier_a_consented_cloud_upgrade_is_used() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let ledger = ConsentLedger::new();
    ledger
        .request(
            &monitor,
            &root,
            1,
            DataScope::Capability("acme-cloud".to_string()),
            "vision generation on this device",
            None,
            1_000,
        )
        .unwrap();

    // The local-tier substitution never fits (simulating no compatible
    // cheaper tier exists at all), so the walk falls through to the
    // consented cloud upgrade.
    let plan = degrade_capability(
        Some(&heavy_vision_policy()),
        &sbc_profile(),
        &ledger,
        1,
        |_| false,
        1_000,
    );
    assert_eq!(
        plan.outcome,
        DegradationOutcome::Substituted {
            substitution: Substitution::ConsentedCloudUpgrade("acme-cloud".to_string())
        }
    );
}

#[test]
fn without_consent_the_cloud_upgrade_is_skipped_and_the_capability_disables() {
    let ledger = ConsentLedger::new(); // no standing grant registered
    let plan = degrade_capability(
        Some(&heavy_vision_policy()),
        &sbc_profile(),
        &ledger,
        1,
        |_| false,
        1_000,
    );
    assert_eq!(plan.outcome, DegradationOutcome::Disabled);
}

#[test]
fn a_policy_with_no_fallback_order_at_all_disables_immediately_on_violation() {
    let ledger = ConsentLedger::new();
    let policy = DegradationPolicy {
        capability_ref: "vision.generate".to_string(),
        constraint: ResourceConstraint {
            min_ram_mb: 999_999,
            min_vram_mb: 0,
            min_compute_tops: 0,
        },
        fallback_order: vec![],
    };
    let plan = degrade_capability(
        Some(&policy),
        &workstation_profile(),
        &ledger,
        1,
        |_| true,
        1_000,
    );
    assert_eq!(plan.outcome, DegradationOutcome::Disabled);
}
