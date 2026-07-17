//! docs/16 §5/§7's `route_capability_call`: deny-by-default, "never
//! assume consent," and residency structurally forbidding cloud for
//! `Restricted` objects even under `CloudAssisted`.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_privacy::{
    route_capability_call, ConsentLedger, DataScope, DegradeReason, PrivacyProfile, PrivacyTier,
    ResidencyTag, RoutingDecision, SensitivityClass,
};

fn profile(tier: PrivacyTier) -> PrivacyProfile {
    PrivacyProfile {
        tier,
        domain_overrides: Default::default(),
        updated_at: 0,
        version: 1,
    }
}

#[test]
fn fully_local_with_a_local_implementation_dispatches_local() {
    let ledger = ConsentLedger::new();
    let decision = route_capability_call(
        &profile(PrivacyTier::FullyLocal),
        "notes",
        &DataScope::Domain("notes".to_string()),
        None,
        true,
        &ledger,
        1,
        1_000,
    );
    assert_eq!(decision, RoutingDecision::DispatchLocal);
}

#[test]
fn fully_local_with_no_local_implementation_degrades_never_escalates_to_cloud() {
    let ledger = ConsentLedger::new();
    let decision = route_capability_call(
        &profile(PrivacyTier::FullyLocal),
        "notes",
        &DataScope::Domain("notes".to_string()),
        None,
        false,
        &ledger,
        1,
        1_000,
    );
    assert_eq!(
        decision,
        RoutingDecision::Degraded(DegradeReason::NoLocalImplementation)
    );
}

#[test]
fn local_preferred_with_consent_falls_back_to_local_when_available() {
    let ledger = ConsentLedger::new();
    let decision = route_capability_call(
        &profile(PrivacyTier::LocalPreferredWithConsent),
        "notes",
        &DataScope::Domain("notes".to_string()),
        None,
        true,
        &ledger,
        1,
        1_000,
    );
    assert_eq!(decision, RoutingDecision::DispatchLocal);
}

#[test]
fn local_preferred_with_consent_and_no_standing_grant_degrades_never_assumes_consent() {
    let ledger = ConsentLedger::new();
    let decision = route_capability_call(
        &profile(PrivacyTier::LocalPreferredWithConsent),
        "notes",
        &DataScope::Domain("notes".to_string()),
        None,
        false,
        &ledger,
        1,
        1_000,
    );
    assert_eq!(
        decision,
        RoutingDecision::Degraded(DegradeReason::NoStandingConsent)
    );
}

#[test]
fn local_preferred_with_a_standing_grant_dispatches_cloud() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let ledger = ConsentLedger::new();
    let device_key = Keystore::ephemeral();
    let scope = DataScope::Domain("notes".to_string());
    let grant = ledger
        .request(
            &monitor,
            &root,
            1,
            scope.clone(),
            "summarize notes",
            None,
            1_000,
            &device_key,
        )
        .unwrap();

    let decision = route_capability_call(
        &profile(PrivacyTier::LocalPreferredWithConsent),
        "notes",
        &scope,
        None,
        false,
        &ledger,
        1,
        1_000,
    );
    assert_eq!(
        decision,
        RoutingDecision::DispatchCloud { grant_id: grant.id }
    );
}

#[test]
fn a_revoked_grant_no_longer_stands_and_the_call_degrades() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let ledger = ConsentLedger::new();
    let device_key = Keystore::ephemeral();
    let scope = DataScope::Domain("notes".to_string());
    let grant = ledger
        .request(
            &monitor,
            &root,
            1,
            scope.clone(),
            "summarize notes",
            None,
            1_000,
            &device_key,
        )
        .unwrap();
    ledger.revoke(&monitor, &root, grant.id).unwrap();

    let decision = route_capability_call(
        &profile(PrivacyTier::LocalPreferredWithConsent),
        "notes",
        &scope,
        None,
        false,
        &ledger,
        1,
        1_000,
    );
    assert_eq!(
        decision,
        RoutingDecision::Degraded(DegradeReason::NoStandingConsent)
    );
}

#[test]
fn an_expired_grant_no_longer_stands() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let ledger = ConsentLedger::new();
    let device_key = Keystore::ephemeral();
    let scope = DataScope::Domain("notes".to_string());
    ledger
        .request(
            &monitor,
            &root,
            1,
            scope.clone(),
            "summarize notes",
            Some(1_500),
            1_000,
            &device_key,
        )
        .unwrap();

    let decision = route_capability_call(
        &profile(PrivacyTier::LocalPreferredWithConsent),
        "notes",
        &scope,
        None,
        false,
        &ledger,
        1,
        1_600,
    );
    assert_eq!(
        decision,
        RoutingDecision::Degraded(DegradeReason::NoStandingConsent)
    );
}

#[test]
fn a_restricted_object_forbids_cloud_even_under_cloud_assisted_tier() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let ledger = ConsentLedger::new();
    let device_key = Keystore::ephemeral();
    let scope = DataScope::Object(hyperion_storage::ObjectId(1));
    ledger
        .request(
            &monitor,
            &root,
            1,
            scope.clone(),
            "anything",
            None,
            1_000,
            &device_key,
        )
        .unwrap();

    let residency = ResidencyTag::new(
        hyperion_storage::ObjectId(1),
        SensitivityClass::Restricted,
        [
            PrivacyTier::FullyLocal,
            PrivacyTier::LocalPreferredWithConsent,
            PrivacyTier::CloudAssisted,
        ]
        .into_iter()
        .collect(),
    );
    // Even though a standing grant exists, residency structurally forbids cloud for Restricted.
    assert!(residency.forbids(PrivacyTier::CloudAssisted));

    let decision = route_capability_call(
        &profile(PrivacyTier::CloudAssisted),
        "health",
        &scope,
        Some(&residency),
        false,
        &ledger,
        1,
        1_000,
    );
    assert_eq!(
        decision,
        RoutingDecision::Degraded(DegradeReason::ResidencyForbidsCloud)
    );
}

#[test]
fn cloud_assisted_with_a_standing_grant_and_permissive_residency_dispatches_cloud() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let ledger = ConsentLedger::new();
    let device_key = Keystore::ephemeral();
    let scope = DataScope::Domain("weather".to_string());
    let grant = ledger
        .request(
            &monitor,
            &root,
            1,
            scope.clone(),
            "get forecast",
            None,
            1_000,
            &device_key,
        )
        .unwrap();

    let decision = route_capability_call(
        &profile(PrivacyTier::CloudAssisted),
        "weather",
        &scope,
        None,
        false,
        &ledger,
        1,
        1_000,
    );
    assert_eq!(
        decision,
        RoutingDecision::DispatchCloud { grant_id: grant.id }
    );
}

#[test]
fn domain_overrides_take_priority_over_the_profiles_default_tier() {
    let ledger = ConsentLedger::new();
    let mut profile = profile(PrivacyTier::CloudAssisted);
    profile
        .domain_overrides
        .insert("health".to_string(), PrivacyTier::FullyLocal);

    let decision = route_capability_call(
        &profile,
        "health",
        &DataScope::Domain("health".to_string()),
        None,
        true,
        &ledger,
        1,
        1_000,
    );
    assert_eq!(decision, RoutingDecision::DispatchLocal);
}
