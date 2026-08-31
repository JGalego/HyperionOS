//! Proving who somebody is (docs/998-roadmap.md's App Builder T4).

use hyperion_crypto::{
    hash_passphrase, verify_passphrase, CredentialError, Keystore, PassphraseVerifier,
};

fn device() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

#[test]
fn the_right_passphrase_verifies_and_a_wrong_one_does_not() {
    let (_dir, device_key) = device();
    let stored = hash_passphrase("correct horse battery", &device_key).unwrap();

    assert!(verify_passphrase(
        "correct horse battery",
        &stored,
        &device_key
    ));
    assert!(!verify_passphrase(
        "correct horse batterz",
        &stored,
        &device_key
    ));
    assert!(!verify_passphrase("", &stored, &device_key));
}

#[test]
fn the_passphrase_itself_is_never_in_what_is_stored() {
    // The stored form goes on disk. If it contained the passphrase, everything else here would be
    // beside the point.
    let (_dir, device_key) = device();
    let stored = hash_passphrase("correct horse battery", &device_key).unwrap();
    assert!(!stored.as_str().contains("correct horse battery"));
    assert!(stored.as_str().starts_with("$argon2id$"));
}

#[test]
fn the_same_passphrase_stores_differently_every_time() {
    // A fresh random salt per credential: two people who choose the same passphrase must not have
    // matching stored verifiers, or the file itself reveals that they match.
    let (_dir, device_key) = device();
    let alice = hash_passphrase("correct horse battery", &device_key).unwrap();
    let bob = hash_passphrase("correct horse battery", &device_key).unwrap();

    assert_ne!(alice.as_str(), bob.as_str());
    // ...and both still verify.
    assert!(verify_passphrase(
        "correct horse battery",
        &alice,
        &device_key
    ));
    assert!(verify_passphrase(
        "correct horse battery",
        &bob,
        &device_key
    ));
}

#[test]
fn a_stolen_credentials_file_is_useless_without_the_device_key() {
    // What the pepper buys. The salt is in the stored string, so an attacker holding the file can
    // normally guess offline; peppering with a device-bound secret means they need the device's
    // signing key too -- the one secret this workspace already treats as the root of everything.
    let (_dir_a, alices_device) = device();
    let (_dir_b, another_device) = device();

    let stored = hash_passphrase("correct horse battery", &alices_device).unwrap();
    assert!(verify_passphrase(
        "correct horse battery",
        &stored,
        &alices_device
    ));
    assert!(
        !verify_passphrase("correct horse battery", &stored, &another_device),
        "the right passphrase must not verify against a different device's key"
    );
}

#[test]
fn a_passphrase_too_short_to_be_worth_hashing_is_refused() {
    // Argon2id makes each guess expensive, which does nothing for a passphrase inside the first
    // thousand guesses. Refused outright rather than warned about.
    let (_dir, device_key) = device();
    assert_eq!(
        hash_passphrase("short", &device_key).unwrap_err(),
        CredentialError::TooShort
    );
    assert!(hash_passphrase("just long", &device_key).is_ok());
}

#[test]
fn length_is_counted_in_characters_rather_than_bytes() {
    // Seven multi-byte characters are still seven characters. A byte-length check would have
    // accepted this.
    let (_dir, device_key) = device();
    assert_eq!(
        hash_passphrase("привет!", &device_key).unwrap_err(),
        CredentialError::TooShort
    );
}

#[test]
fn a_stored_verifier_survives_a_round_trip_through_disk() {
    let (_dir, device_key) = device();
    let stored = hash_passphrase("correct horse battery", &device_key).unwrap();

    let reloaded = PassphraseVerifier::parse(stored.as_str()).expect("what we wrote must parse");
    assert!(verify_passphrase(
        "correct horse battery",
        &reloaded,
        &device_key
    ));
}

#[test]
fn a_damaged_verifier_is_refused_at_load_rather_than_at_login() {
    // Better to notice a corrupt credentials file when reading it than to have somebody's correct
    // passphrase mysteriously stop working.
    assert_eq!(
        PassphraseVerifier::parse("not a real hash").unwrap_err(),
        CredentialError::Malformed
    );
    assert_eq!(
        PassphraseVerifier::parse("").unwrap_err(),
        CredentialError::Malformed
    );
}
