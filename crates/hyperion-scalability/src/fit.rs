use hyperion_scheduler::{ResourceDimension, Scheduler};

use crate::types::{CapacityDescriptor, Substitution};

/// A real `resolve_alternate_fits`-shaped fit-check backed by
/// `hyperion_scheduler::Scheduler`'s live ledgers — the `scheduler.would_fit(...)`
/// [`crate::degrade::degrade_capability`]'s own doc comment names as what
/// its caller-supplied callback stands in for. Checks the two of this
/// crate's three [`CapacityDescriptor`] dimensions the scheduler's
/// `ResourceVector` actually models the same way — `ram_mb`/`vram_mb`
/// against the scheduler's real `Ram`/`Vram` ledgers' current headroom.
/// `compute_tops` is not checked: docs/37's raw hardware TOPS ceiling has
/// no faithful counterpart among the scheduler's nine dimensions (`Gpu`
/// is a share-based allocation unit, `InferenceTokens` a per-task
/// throughput request — neither is "device compute capacity" in the
/// sense this crate's tier table means), so forcing a mapping there would
/// silently misreport fit rather than leave the gap honest. This is
/// exactly the case this crate's doc comment describes: real scheduler
/// state for the dimensions that line up, not full nine-dimension parity.
pub fn scheduler_has_headroom_for(scheduler: &Scheduler, footprint: &CapacityDescriptor) -> bool {
    dimension_fits(scheduler, ResourceDimension::Ram, footprint.ram_mb)
        && dimension_fits(scheduler, ResourceDimension::Vram, footprint.vram_mb)
}

fn dimension_fits(scheduler: &Scheduler, dim: ResourceDimension, want: u32) -> bool {
    scheduler
        .query_ledger(dim)
        .is_some_and(|ledger| ledger.allocated.saturating_add(want) <= ledger.headroom(false))
}

/// Builds a real `resolve_alternate_fits` closure ready to hand straight
/// to [`crate::degrade::degrade_capability`], backed by
/// [`scheduler_has_headroom_for`]. This crate's own [`Substitution`]
/// carries no resource footprint (`CheaperLocalTier`/`AlternateImplementation`
/// name a tier or capability, not a `CapacityDescriptor`), so the caller
/// supplies `footprint_for` to look one up (e.g. from a `ModelTier` ->
/// `CapacityDescriptor` table it maintains); a substitution this lookup
/// can't resolve never fits, matching `degrade_capability`'s existing
/// deny-by-default fallback-order walk.
pub fn scheduler_backed_resolver<'a>(
    scheduler: &'a Scheduler,
    footprint_for: impl Fn(&Substitution) -> Option<CapacityDescriptor> + 'a,
) -> impl Fn(&Substitution) -> bool + 'a {
    move |substitution| {
        footprint_for(substitution)
            .is_some_and(|footprint| scheduler_has_headroom_for(scheduler, &footprint))
    }
}
