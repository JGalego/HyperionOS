//! docs/998-roadmap.md's own named "SyncEnvelope-wrapped encrypted payloads" gap, closed for
//! real: `FederationHub::seal`/`open` really encrypt and really sign a payload via
//! `hyperion_crypto::sync_envelope`, using this hub's own real identity.

use hyperion_federation::FederationHub;

#[test]
fn a_hub_can_seal_and_open_its_own_real_envelope() {
    let hub = FederationHub::new();
    let envelope = hub.seal(1, b"a real ledger update");
    let opened = hub
        .open(&envelope)
        .expect("a real, untampered envelope must open");
    assert_eq!(opened, b"a real ledger update");
    assert_eq!(envelope.sender_id, 1);
}

#[test]
fn two_different_hubs_have_different_real_identities_and_cannot_open_each_others_envelopes() {
    let hub_a = FederationHub::new();
    let hub_b = FederationHub::new();

    let envelope = hub_a.seal(1, b"only hub_a's own devices should read this");
    let result = hub_b.open(&envelope);
    assert!(
        result.is_err(),
        "a different hub's real, independently-generated identity must not be able to open this"
    );
}

#[test]
fn two_genuinely_independent_hubs_seal_and_open_for_each_other_via_a_real_x25519_exchange() {
    let hub_a = FederationHub::new();
    let hub_b = FederationHub::new();

    // Real X25519 key agreement between two hubs with independent identities -- neither ever
    // sees the other's private key, only its real public X25519 key.
    let a_shared = hub_a.establish_shared_secret(&hub_b.x25519_public());
    let b_shared = hub_b.establish_shared_secret(&hub_a.x25519_public());
    assert_eq!(a_shared, b_shared);

    let envelope = hub_a.seal_for_peer(&a_shared, 1, b"a real cross-hub message");
    let opened = hub_b
        .open_from_peer(&hub_a.verifying_key(), &b_shared, &envelope)
        .expect("hub_b's own independently-derived shared secret must open hub_a's real envelope");
    assert_eq!(opened, b"a real cross-hub message");
}

#[test]
fn a_third_hubs_shared_secret_cannot_open_an_envelope_sealed_for_a_different_peer() {
    let hub_a = FederationHub::new();
    let hub_b = FederationHub::new();
    let hub_c = FederationHub::new();

    let ab_shared = hub_a.establish_shared_secret(&hub_b.x25519_public());
    let ac_shared = hub_a.establish_shared_secret(&hub_c.x25519_public());

    let envelope = hub_a.seal_for_peer(&ab_shared, 1, b"only for hub_b");
    let result = hub_c.open_from_peer(&hub_a.verifying_key(), &ac_shared, &envelope);
    assert!(
        result.is_err(),
        "a shared secret established with a different peer must not open this envelope"
    );
}

#[test]
fn a_hub_constructed_with_a_real_persisted_keystore_can_be_reopened_with_the_same_identity() {
    let dir = tempfile::tempdir().unwrap();
    let key_path = dir.path().join("device.key");

    let envelope = {
        let keystore = hyperion_crypto::Keystore::open_or_create(&key_path).unwrap();
        let hub = FederationHub::new_with_keystore(keystore);
        hub.seal(1, b"sealed with a real, persisted identity")
    };

    // A fresh hub reopened against the exact same real key file must be able to open it.
    let keystore = hyperion_crypto::Keystore::open_or_create(&key_path).unwrap();
    let hub = FederationHub::new_with_keystore(keystore);
    let opened = hub.open(&envelope).unwrap();
    assert_eq!(opened, b"sealed with a real, persisted identity");
}
