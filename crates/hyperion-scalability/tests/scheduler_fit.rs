//! Proves docs/37 §3's `resolve_alternate_fits` callback can be backed by
//! the real `hyperion_scheduler::Scheduler`, not just a caller-supplied
//! closure with no real resource state behind it.

use hyperion_privacy::ConsentLedger;
use hyperion_scalability::{
    degrade_capability, scheduler_backed_resolver, scheduler_has_headroom_for, CapacityDescriptor,
    DegradationOutcome, DegradationPolicy, HardwareProfile, HardwareTier, ModelTier,
    ResourceConstraint, Substitution, TenancyMode,
};
use hyperion_scheduler::{ResourceDimension, ResourceLedger, Scheduler};

fn scheduler_with_headroom(ram_mb: u32, vram_mb: u32) -> Scheduler {
    let mut scheduler = Scheduler::new();
    scheduler.register_resource_provider(ResourceLedger::new(ResourceDimension::Ram, ram_mb, 0));
    scheduler.register_resource_provider(ResourceLedger::new(ResourceDimension::Vram, vram_mb, 0));
    scheduler
}

#[test]
fn scheduler_has_headroom_for_reflects_the_real_ledgers() {
    let scheduler = scheduler_with_headroom(8_192, 4_096);

    assert!(scheduler_has_headroom_for(
        &scheduler,
        &CapacityDescriptor {
            ram_mb: 8_192,
            vram_mb: 4_096,
            compute_tops: 999, // deliberately not checked -- see fit's doc comment
        }
    ));
    assert!(!scheduler_has_headroom_for(
        &scheduler,
        &CapacityDescriptor {
            ram_mb: 8_193,
            vram_mb: 0,
            compute_tops: 0,
        }
    ));
    assert!(!scheduler_has_headroom_for(
        &scheduler,
        &CapacityDescriptor {
            ram_mb: 0,
            vram_mb: 4_097,
            compute_tops: 0,
        }
    ));
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
            Substitution::Disable,
        ],
    }
}

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

/// `TinyEdge`'s real resource footprint, standing in for a table a real
/// caller would maintain -- this crate's own `Substitution` has no
/// footprint field (see `fit`'s doc comment).
fn footprint_for(substitution: &Substitution) -> Option<CapacityDescriptor> {
    match substitution {
        Substitution::CheaperLocalTier(ModelTier::TinyEdge) => Some(CapacityDescriptor {
            ram_mb: 2_048,
            vram_mb: 0,
            compute_tops: 2,
        }),
        _ => None,
    }
}

#[test]
fn degrade_capability_substitutes_when_the_real_scheduler_has_room() {
    let scheduler = scheduler_with_headroom(2_048, 0);
    let ledger = ConsentLedger::new();

    let plan = degrade_capability(
        Some(&heavy_vision_policy()),
        &sbc_profile(),
        &ledger,
        1,
        scheduler_backed_resolver(&scheduler, footprint_for),
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
fn degrade_capability_disables_when_the_real_scheduler_has_no_room() {
    // Real ledger capacity is smaller than TinyEdge's real footprint --
    // an actual fit-check, not a stubbed yes/no.
    let scheduler = scheduler_with_headroom(1_024, 0);
    let ledger = ConsentLedger::new();

    let plan = degrade_capability(
        Some(&heavy_vision_policy()),
        &sbc_profile(),
        &ledger,
        1,
        scheduler_backed_resolver(&scheduler, footprint_for),
        1_000,
    );

    assert_eq!(plan.outcome, DegradationOutcome::Disabled);
}
