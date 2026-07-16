//! docs/998-roadmap.md's own named "heartbeat timing" gap
//! (`hyperion-federation`'s "real network transport, heartbeat timing, ambient anti-entropy"
//! deferred list): `FederationHub::start_lease_heartbeat` really renews a lease automatically, on
//! a real wall-clock interval, without a caller ever calling `renew_lease` itself.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_federation::{FederationHub, FederationTrustTier};

fn real_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn setup() -> (
    Arc<CapabilityMonitor>,
    hyperion_capability::CapabilityToken,
    Arc<FederationHub>,
) {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let hub = Arc::new(FederationHub::new());
    hub.join_device(&monitor, &token, 1, FederationTrustTier::OwnedPrimary)
        .unwrap();
    (Arc::new(monitor), token, hub)
}

#[test]
fn a_real_heartbeat_keeps_a_short_lived_lease_alive_past_its_original_ttl() {
    let (monitor, token, hub) = setup();

    let now = real_now();
    // A one-second ttl -- without renewal, this lease would go stale almost immediately.
    hub.acquire_lease(&monitor, &token, 42, 1, now, 1).unwrap();

    let heartbeat = hub.start_lease_heartbeat(
        Arc::clone(&monitor),
        token.clone(),
        42,
        1,
        Duration::from_millis(150),
    );

    // Real wall-clock sleep, well past the original 1-second ttl -- only a genuinely running
    // heartbeat (not just a generous ttl) can keep this lease looking fresh afterward.
    std::thread::sleep(Duration::from_millis(1_200));

    let lease = hub
        .lease_of(42)
        .expect("the lease must still exist after real renewal");
    let fresh_now = real_now();
    assert!(
        fresh_now.saturating_sub(lease.granted_at) <= lease.ttl_secs,
        "a real, running heartbeat must have renewed this lease recently enough to still be \
         live, got granted_at={} fresh_now={}",
        lease.granted_at,
        fresh_now
    );

    heartbeat.stop();
}

#[test]
fn without_a_heartbeat_a_short_lived_lease_really_goes_stale() {
    let (monitor, token, hub) = setup();

    let now = real_now();
    hub.acquire_lease(&monitor, &token, 42, 1, now, 1).unwrap();

    // A generous 2.5s margin -- see `stopping_a_heartbeat_really_stops_it_from_renewing_further`'s
    // own comment on why a shorter one risks flaking on whole-second rounding, not real behavior.
    std::thread::sleep(Duration::from_millis(2_500));

    let lease = hub
        .lease_of(42)
        .expect("the lease record itself still exists");
    let fresh_now = real_now();
    assert!(
        fresh_now.saturating_sub(lease.granted_at) > lease.ttl_secs,
        "with no heartbeat at all, a one-second-ttl lease must genuinely go stale after 2.5 \
         real seconds"
    );
}

#[test]
fn stopping_a_heartbeat_really_stops_it_from_renewing_further() {
    let (monitor, token, hub) = setup();

    let now = real_now();
    hub.acquire_lease(&monitor, &token, 42, 1, now, 1).unwrap();

    let heartbeat = hub.start_lease_heartbeat(
        Arc::clone(&monitor),
        token.clone(),
        42,
        1,
        Duration::from_millis(150),
    );
    std::thread::sleep(Duration::from_millis(400));
    heartbeat.stop();

    let granted_at_after_stop = hub.lease_of(42).unwrap().granted_at;

    // No more renewal should happen after a real stop -- sleeping well past the ttl again must
    // let the lease go stale, proving `stop` genuinely halted the real background thread rather
    // than merely detaching it. A generous 2.5s (not just ~1.2s) margin, since `granted_at`/`now`
    // are whole real seconds -- a shorter margin can floor to exactly the ttl at an unlucky
    // second boundary and make this assertion flaky for a reason that has nothing to do with the
    // real behavior under test.
    std::thread::sleep(Duration::from_millis(2_500));
    let lease = hub.lease_of(42).unwrap();
    assert_eq!(
        lease.granted_at, granted_at_after_stop,
        "no renewal should have happened after a real stop"
    );
    let fresh_now = real_now();
    assert!(
        fresh_now.saturating_sub(lease.granted_at) > lease.ttl_secs,
        "a stopped heartbeat must let the lease genuinely go stale"
    );
}
