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
