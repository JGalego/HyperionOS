//! docs/998-roadmap.md's Social pillar: a real, persisted trust-on-first-use peer identity
//! store -- the first contact with a peer id records its key for real; a later contact with the
//! same key is silently confirmed; a later contact with a *different* key is a real, surfaced
//! mismatch, never silently overwritten.

use hyperion_console::peer_trust::{decode_hex, encode_hex, PeerTrustStore, TrustOutcome};

#[test]
fn a_hex_round_trip_is_exact() {
    let bytes = [0u8, 1, 254, 255, 16, 32];
    let hex = encode_hex(&bytes);
    assert_eq!(hex, "0001feff1020");
    assert_eq!(decode_hex(&hex).unwrap(), bytes);
}

#[test]
fn an_odd_length_or_non_hex_string_fails_to_decode() {
    assert!(decode_hex("abc").is_none());
    assert!(decode_hex("zz").is_none());
}

#[test]
fn the_first_contact_with_a_peer_is_recorded_as_first_trust() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PeerTrustStore::open_or_create(dir.path().join("peer_trust.json")).unwrap();

    let outcome = store.verify_or_trust("127.0.0.1:9000", "aabbcc").unwrap();
    assert_eq!(outcome, TrustOutcome::FirstTrust);
}

#[test]
fn the_same_key_on_a_later_contact_is_silently_confirmed() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PeerTrustStore::open_or_create(dir.path().join("peer_trust.json")).unwrap();

    store.verify_or_trust("127.0.0.1:9000", "aabbcc").unwrap();
    let outcome = store.verify_or_trust("127.0.0.1:9000", "aabbcc").unwrap();
    assert_eq!(outcome, TrustOutcome::Trusted);
}

#[test]
fn a_different_key_on_a_later_contact_is_a_real_surfaced_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PeerTrustStore::open_or_create(dir.path().join("peer_trust.json")).unwrap();

    store.verify_or_trust("127.0.0.1:9000", "aabbcc").unwrap();
    let outcome = store.verify_or_trust("127.0.0.1:9000", "ddeeff").unwrap();
    assert_eq!(
        outcome,
        TrustOutcome::KeyMismatch {
            previously_trusted_key_hex: "aabbcc".to_string()
        }
    );
}

#[test]
fn trust_really_survives_a_fresh_store_re_opened_from_the_same_real_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("peer_trust.json");
    {
        let mut store = PeerTrustStore::open_or_create(&path).unwrap();
        store.verify_or_trust("127.0.0.1:9000", "aabbcc").unwrap();
    }

    let mut reopened = PeerTrustStore::open_or_create(&path).unwrap();
    let outcome = reopened
        .verify_or_trust("127.0.0.1:9000", "aabbcc")
        .unwrap();
    assert_eq!(outcome, TrustOutcome::Trusted);
}

#[test]
fn forgetting_a_peer_really_clears_it_so_the_next_contact_is_first_trust_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PeerTrustStore::open_or_create(dir.path().join("peer_trust.json")).unwrap();

    store.verify_or_trust("127.0.0.1:9000", "aabbcc").unwrap();
    assert!(store.forget("127.0.0.1:9000").unwrap());
    assert!(!store.forget("127.0.0.1:9000").unwrap(), "already gone");

    let outcome = store.verify_or_trust("127.0.0.1:9000", "ddeeff").unwrap();
    assert_eq!(outcome, TrustOutcome::FirstTrust);
}

#[test]
fn trusted_peers_lists_every_real_entry_sorted_by_id() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PeerTrustStore::open_or_create(dir.path().join("peer_trust.json")).unwrap();

    store.verify_or_trust("b-host:9000", "bb").unwrap();
    store.verify_or_trust("a-host:9000", "aa").unwrap();

    assert_eq!(
        store.trusted_peers(),
        vec![
            ("a-host:9000".to_string(), "aa".to_string()),
            ("b-host:9000".to_string(), "bb".to_string()),
        ]
    );
}
