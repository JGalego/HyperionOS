use crate::types::ResourceVector;

/// Cumulative resource consumption *currently held* by one scheduling owner
/// — not a historical sum — per docs/04-scheduler.md §Data Structures. This
/// is the quantity real Dominant Resource Fairness ranks by: tracking it
/// per-owner (not per-task) is what makes DRF strategy-proof, since an
/// owner cannot gain share by splitting one large request into many small
/// ones — `currently_held` sums across every in-flight task that owner has,
/// however it's split.
///
/// The spec's struct also carries a redundant `owner: OwnerId` field; this
/// is dropped here since [`crate::scheduler::Scheduler`] always stores this
/// behind a `HashMap<OwnerId, OwnerAccount>`, so the map key already *is*
/// the owner and duplicating it inside the value could only ever drift out
/// of sync with its own key.
#[derive(Debug, Default, Clone, Copy)]
pub struct OwnerAccount {
    pub currently_held: ResourceVector,
}
