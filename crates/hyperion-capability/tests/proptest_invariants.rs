//! Property-based tests proving the three invariants
//! docs/03-kernel-architecture.md claims for capability derivation and
//! revocation (per docs/35-testing-strategy.md, L0-L2 correctness is
//! non-negotiable and deterministically/property-tested, never left to
//! probabilistic evaluation):
//!
//! 1. A derived token's rights/expiry are always within its parent's.
//! 2. Revoking a token invalidates every descendant (and nothing else).
//! 3. A stale-generation token is always rejected.

use std::time::Duration;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use proptest::prelude::*;

fn rights_strategy() -> impl Strategy<Value = RightsMask> {
    (0u32..=RightsMask::all().bits()).prop_map(RightsMask::from_bits_truncate)
}

/// A random subset of `parent`'s bits — the only rights value `cap_derive`
/// should ever be able to produce from it.
fn attenuated_subset(parent: RightsMask) -> impl Strategy<Value = RightsMask> {
    let bits = parent.bits();
    (0u32..=bits).prop_map(move |mask| RightsMask::from_bits_truncate(mask & bits))
}

/// Pairs a random rights mask with a random attenuated subset of it, so a
/// single `proptest!` parameter yields both halves already related the way
/// `cap_derive` requires — no manual `TestRunner` needed inside a test body.
fn parent_and_attenuated_child() -> impl Strategy<Value = (RightsMask, RightsMask)> {
    rights_strategy()
        .prop_flat_map(|parent| attenuated_subset(parent).prop_map(move |child| (parent, child)))
}

fn ttl_strategy() -> impl Strategy<Value = Option<Duration>> {
    prop_oneof![
        Just(None),
        (1u64..1_000_000).prop_map(|ms| Some(Duration::from_millis(ms))),
    ]
}

proptest! {
    /// Invariant 1: a derived token's rights are always a subset of its
    /// parent's, and its expiry is never later than its parent's.
    #[test]
    fn derived_token_is_always_within_parent_bounds(
        (parent_rights, child_rights) in parent_and_attenuated_child(),
        parent_ttl in ttl_strategy(),
        child_ttl in ttl_strategy(),
    ) {
        let mut m = CapabilityMonitor::new();
        let root = m.mint_root(parent_rights, TrustBoundaryId(1), parent_ttl);

        let child = m
            .cap_derive(&root, child_rights, child_ttl, TrustBoundaryId(2))
            .expect("attenuation-only derivation must succeed for a rights subset");

        prop_assert!(parent_rights.contains(child.rights()));
        prop_assert_eq!(child.object_id(), root.object_id());

        match (root.expiry(), child.expiry()) {
            (None, _) => {}                          // parent has no deadline: child's is unconstrained
            (Some(p), Some(c)) => prop_assert!(c <= p),
            (Some(_), None) => prop_assert!(false, "child must inherit a bounded parent expiry"),
        }
    }

    /// Attempting to derive rights outside the parent's set must always be
    /// rejected, never silently clamped or allowed.
    #[test]
    fn derivation_can_never_escalate_rights(
        parent_rights in rights_strategy(),
        requested_rights in rights_strategy(),
    ) {
        let mut m = CapabilityMonitor::new();
        let root = m.mint_root(parent_rights, TrustBoundaryId(1), None);
        let result = m.cap_derive(&root, requested_rights, None, TrustBoundaryId(2));

        if parent_rights.contains(requested_rights) {
            prop_assert!(result.is_ok());
        } else {
            prop_assert!(result.is_err());
        }
    }

    /// Invariant 2 + 3, over a randomly generated delegation tree: revoking
    /// one node invalidates exactly its descendants (stale generation, per
    /// invariant 3) and leaves every token outside that subtree live.
    #[test]
    fn revocation_invalidates_exactly_the_subtree(
        chain_len in 1usize..8,
        revoke_at in 0usize..8,
    ) {
        let mut m = CapabilityMonitor::new();
        let revoke_at = revoke_at % chain_len;

        // Build one linear delegation chain root -> t1 -> t2 -> ... plus one
        // sibling branching off *before* the revoked node and one *at* it,
        // so the test can distinguish "ancestor/sibling survives" from
        // "descendant dies" precisely.
        let root = m.mint_root(RightsMask::all(), TrustBoundaryId(0), None);
        let mut chain = vec![root.clone()];
        for i in 0..chain_len {
            let parent = chain.last().unwrap();
            let child = m
                .cap_derive(parent, RightsMask::READ, None, TrustBoundaryId(i as u64 + 1))
                .unwrap();
            chain.push(child);
        }
        // chain[0] is the root; chain[k] is derived from chain[k-1].
        let sibling_of_target = m
            .cap_derive(&chain[revoke_at], RightsMask::READ, None, TrustBoundaryId(100))
            .unwrap();

        let target = chain[revoke_at + 1].clone(); // the node we will revoke
        m.cap_revoke(&target);

        // Everything at or before the revoked node (ancestors) stays live.
        for ancestor in &chain[..=revoke_at] {
            prop_assert!(m.is_live(ancestor), "ancestor of revoked node must remain live");
        }
        // A sibling derived from the same parent as the revoked node, but
        // not through it, must be unaffected.
        prop_assert!(m.is_live(&sibling_of_target));

        // The revoked node and everything descended from it must be dead —
        // this is invariant 3 (stale generation always rejected) applied to
        // invariant 2's whole cascaded subtree.
        for descendant in &chain[revoke_at + 1..] {
            prop_assert!(!m.is_live(descendant), "revoked node or its descendant must be dead");
        }
    }
}
