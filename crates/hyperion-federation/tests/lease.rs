//! docs/21 §Algorithms' "Anchor lease" + §Recovery Mechanisms' split-brain
//! tie-break: higher `FederationTrustTier` wins a conflicting claim, ties
//! broken by lower `device_id`; the loser is rejected outright.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_federation::{FederationError, FederationHub, FederationTrustTier};

fn setup() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    FederationHub,
) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let hub = FederationHub::new();
    (monitor, root, hub)
}

#[test]
fn a_fresh_lease_is_granted_at_generation_zero() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    let lease = hub
        .acquire_lease(&monitor, &root, 42, 1, 1_000, 60)
        .unwrap();
    assert_eq!(lease.generation, 0);
    assert_eq!(lease.holder_device, 1);
}

#[test]
fn the_same_device_may_renew_its_own_claim_without_a_generation_bump() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    hub.acquire_lease(&monitor, &root, 42, 1, 1_000, 60)
        .unwrap();
    let lease = hub
        .acquire_lease(&monitor, &root, 42, 1, 1_010, 60)
        .unwrap();
    assert_eq!(lease.generation, 0);
    assert_eq!(lease.holder_device, 1);
}

#[test]
fn a_higher_trust_tier_wins_a_conflicting_claim() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::SharedHousehold)
        .unwrap();
    hub.join_device(&monitor, &root, 2, FederationTrustTier::OwnedPrimary)
        .unwrap();
    hub.acquire_lease(&monitor, &root, 42, 1, 1_000, 60)
        .unwrap();

    let lease = hub
        .acquire_lease(&monitor, &root, 42, 2, 1_005, 60)
        .unwrap();
    assert_eq!(lease.holder_device, 2);
    assert_eq!(lease.generation, 1);
}

#[test]
fn a_lower_trust_tier_is_rejected_by_a_conflicting_claim() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    hub.join_device(&monitor, &root, 2, FederationTrustTier::SharedHousehold)
        .unwrap();
    hub.acquire_lease(&monitor, &root, 42, 1, 1_000, 60)
        .unwrap();

    let result = hub.acquire_lease(&monitor, &root, 42, 2, 1_005, 60);
    assert!(matches!(result, Err(FederationError::LeaseConflict)));
    // The original holder is unaffected by the rejected challenge.
    assert_eq!(hub.lease_of(42).unwrap().holder_device, 1);
}

#[test]
fn equal_trust_tiers_break_ties_by_lower_device_id() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 5, FederationTrustTier::OwnedSecondary)
        .unwrap();
    hub.join_device(&monitor, &root, 3, FederationTrustTier::OwnedSecondary)
        .unwrap();
    hub.acquire_lease(&monitor, &root, 42, 5, 1_000, 60)
        .unwrap();

    // Device 3 has a lower id at equal trust and should win the challenge.
    let lease = hub
        .acquire_lease(&monitor, &root, 42, 3, 1_005, 60)
        .unwrap();
    assert_eq!(lease.holder_device, 3);
}

#[test]
fn an_expired_lease_is_freely_reclaimed_regardless_of_trust_tier() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    hub.join_device(&monitor, &root, 2, FederationTrustTier::CloudRented)
        .unwrap();
    hub.acquire_lease(&monitor, &root, 42, 1, 1_000, 10)
        .unwrap();

    // Well past the 10s ttl.
    let lease = hub
        .acquire_lease(&monitor, &root, 42, 2, 1_100, 60)
        .unwrap();
    assert_eq!(lease.holder_device, 2);
}

#[test]
fn only_the_holder_may_renew() {
    let (monitor, root, hub) = setup();
    hub.join_device(&monitor, &root, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    hub.acquire_lease(&monitor, &root, 42, 1, 1_000, 60)
        .unwrap();

    let result = hub.renew_lease(&monitor, &root, 42, 2, 1_010);
    assert!(matches!(result, Err(FederationError::NotAuthoritative)));
}
