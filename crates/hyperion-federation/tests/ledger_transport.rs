//! docs/998-roadmap.md's own named "actual sockets carrying these envelopes between processes"
//! gap, closed for real: `serve_ledger_publications`/`publish_ledger_over_socket` really move a
//! `LedgerPublication` between two genuinely independent `FederationHub` instances over a real
//! `TcpStream`, `seal_for_peer`-encrypted and signed the whole way.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_federation::{
    publish_ledger_over_socket, serve_ledger_publications, FederationHub, FederationTrustTier,
    LedgerPublication,
};
use hyperion_scheduler::ResourceVector;

fn real_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn monitor_and_token() -> (CapabilityMonitor, hyperion_capability::CapabilityToken) {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    (monitor, token)
}

/// Polls for up to two real seconds -- a real network handoff across a background thread has no
/// guaranteed instant delivery, even on loopback.
fn wait_for_ledger(
    hub: &FederationHub,
    device_id: u64,
) -> Option<hyperion_federation::VirtualResourceLedger> {
    for _ in 0..200 {
        if let Some(ledger) = hub.ledger_of(device_id) {
            return Some(ledger);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    None
}

#[test]
fn a_ledger_publication_really_travels_over_a_real_socket_between_two_independent_hubs() {
    let hub_a = Arc::new(FederationHub::new());
    let hub_b = Arc::new(FederationHub::new());
    let (monitor, token) = monitor_and_token();

    // hub_b must already know about device 7 (hub_a's device) before it will accept a ledger
    // published on its behalf -- the same prerequisite `publish_ledger` always had.
    hub_b
        .join_device(&monitor, &token, 7, FederationTrustTier::SharedHousehold)
        .unwrap();

    let shared_secret_a = hub_a.establish_shared_secret(&hub_b.x25519_public());
    let shared_secret_b = hub_b.establish_shared_secret(&hub_a.x25519_public());
    assert_eq!(shared_secret_a, shared_secret_b);

    let server = serve_ledger_publications(
        Arc::clone(&hub_b),
        "127.0.0.1:0",
        hub_a.verifying_key(),
        shared_secret_b,
    )
    .expect("binding a real loopback TCP listener must succeed");

    let publication = LedgerPublication {
        device_id: 7,
        available: ResourceVector {
            cpu_shares: 4,
            ram_mb: 2_048,
            ..Default::default()
        },
        network_latency_ms: 15,
        ttl_secs: 120,
    };
    publish_ledger_over_socket(
        &hub_a,
        &server.local_addr().to_string(),
        &shared_secret_a,
        publication,
    )
    .expect("connecting to the real, already-bound server must succeed");

    let ledger = wait_for_ledger(&hub_b, 7)
        .expect("the real ledger publication must arrive over the real socket");
    assert_eq!(ledger.device_id, 7);
    assert_eq!(ledger.available.cpu_shares, 4);
    assert_eq!(ledger.available.ram_mb, 2_048);
    assert_eq!(ledger.network_latency_ms, 15);
    assert_eq!(ledger.ttl_secs, 120);
    assert_eq!(ledger.trust_tier, FederationTrustTier::SharedHousehold);
    assert!(
        ledger.published_at.abs_diff(real_now()) < 5,
        "the receiving hub must stamp published_at with its own real wall clock, not a sender-\
         supplied value"
    );

    server.stop();
}

#[test]
fn a_publication_signed_with_the_wrong_verifying_key_is_silently_dropped() {
    let hub_a = Arc::new(FederationHub::new());
    let hub_impostor = Arc::new(FederationHub::new());
    let hub_b = Arc::new(FederationHub::new());
    let (monitor, token) = monitor_and_token();

    hub_b
        .join_device(&monitor, &token, 9, FederationTrustTier::CloudRented)
        .unwrap();

    let shared_secret_impostor_side = hub_impostor.establish_shared_secret(&hub_b.x25519_public());
    let shared_secret_b_side = hub_b.establish_shared_secret(&hub_impostor.x25519_public());

    // hub_b's server expects publications signed by hub_a, not hub_impostor.
    let server = serve_ledger_publications(
        Arc::clone(&hub_b),
        "127.0.0.1:0",
        hub_a.verifying_key(),
        shared_secret_b_side,
    )
    .unwrap();

    let publication = LedgerPublication {
        device_id: 9,
        available: ResourceVector::default(),
        network_latency_ms: 5,
        ttl_secs: 60,
    };
    publish_ledger_over_socket(
        &hub_impostor,
        &server.local_addr().to_string(),
        &shared_secret_impostor_side,
        publication,
    )
    .unwrap();

    // Give the server thread real time to receive and (correctly) reject the connection.
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        hub_b.ledger_of(9).is_none(),
        "a publication signed by the wrong real identity must never be applied"
    );

    server.stop();
}
