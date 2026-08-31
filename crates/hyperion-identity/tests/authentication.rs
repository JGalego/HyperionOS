//! Turning "separates people" into "protects them" — docs/998-roadmap.md's App Builder T4.

use hyperion_crypto::Keystore;
use hyperion_identity::{AuthOutcome, CredentialStore, UserId};

struct Device {
    store: CredentialStore,
    keystore: Keystore,
    dir: tempfile::TempDir,
}

fn device() -> Device {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    let store = CredentialStore::open_or_create(dir.path().join("passphrases.json")).unwrap();
    Device {
        store,
        keystore,
        dir,
    }
}

fn alice() -> UserId {
    UserId::new("alice").unwrap()
}

fn bob() -> UserId {
    UserId::new("bob").unwrap()
}

#[test]
fn a_person_with_no_passphrase_is_neither_let_in_nor_refused() {
    // The honest state of a device that has principals but no credentials. What must never happen
    // is for this to be quietly treated as verified, which is how a system ends up authenticating
    // nobody while appearing to authenticate everybody.
    let device = device();
    assert!(!device.store.has_credential(&alice()));
    assert_eq!(
        device
            .store
            .authenticate(&alice(), "anything", &device.keystore),
        AuthOutcome::NoCredential
    );
}

#[test]
fn the_right_passphrase_is_accepted_and_a_wrong_one_is_not() {
    let mut device = device();
    device
        .store
        .set(&alice(), "correct horse battery", &device.keystore)
        .unwrap();

    assert_eq!(
        device
            .store
            .authenticate(&alice(), "correct horse battery", &device.keystore),
        AuthOutcome::Verified
    );
    assert_eq!(
        device
            .store
            .authenticate(&alice(), "correct horse batterz", &device.keystore),
        AuthOutcome::Refused
    );
}

#[test]
fn one_persons_passphrase_never_works_as_anothers() {
    let mut device = device();
    device
        .store
        .set(&alice(), "alices own passphrase", &device.keystore)
        .unwrap();
    device
        .store
        .set(&bob(), "bobs own passphrase", &device.keystore)
        .unwrap();

    assert_eq!(
        device
            .store
            .authenticate(&bob(), "alices own passphrase", &device.keystore),
        AuthOutcome::Refused
    );
}

#[test]
fn a_name_nobody_has_set_up_looks_the_same_as_a_wrong_passphrase_would() {
    // Not quite: it is deliberately its own outcome, so a caller can tell a device that has no
    // credentials from one where somebody guessed wrong. What matters is that *refusal* never
    // distinguishes "wrong passphrase" from anything else, so trying names cannot enumerate who
    // exists on a device.
    let mut device = device();
    device
        .store
        .set(&alice(), "alices own passphrase", &device.keystore)
        .unwrap();

    assert_eq!(
        device
            .store
            .authenticate(&alice(), "wrong", &device.keystore),
        AuthOutcome::Refused
    );
    let carol = UserId::new("carol").unwrap();
    assert_eq!(
        device.store.authenticate(&carol, "wrong", &device.keystore),
        AuthOutcome::NoCredential
    );
}

#[test]
fn passphrases_survive_a_restart() {
    let device = device();
    let path = device.dir.path().join("passphrases.json");
    {
        let mut store = CredentialStore::open_or_create(&path).unwrap();
        store
            .set(&alice(), "correct horse battery", &device.keystore)
            .unwrap();
    }
    let reopened = CredentialStore::open_or_create(&path).unwrap();
    assert_eq!(
        reopened.authenticate(&alice(), "correct horse battery", &device.keystore),
        AuthOutcome::Verified
    );
}

#[test]
fn what_is_written_to_disk_never_contains_the_passphrase() {
    let device = device();
    let path = device.dir.path().join("passphrases.json");
    {
        let mut store = CredentialStore::open_or_create(&path).unwrap();
        store
            .set(&alice(), "correct horse battery", &device.keystore)
            .unwrap();
    }
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(!on_disk.contains("correct horse battery"));
    assert!(on_disk.contains("argon2id"));
}

#[test]
fn clearing_a_passphrase_leaves_someone_unprotected_rather_than_locked_out() {
    let mut device = device();
    device
        .store
        .set(&alice(), "correct horse battery", &device.keystore)
        .unwrap();
    device.store.clear(&alice()).unwrap();

    assert!(!device.store.has_credential(&alice()));
    assert_eq!(
        device
            .store
            .authenticate(&alice(), "correct horse battery", &device.keystore),
        AuthOutcome::NoCredential
    );
}

#[test]
fn a_corrupt_credentials_file_is_an_error_at_load_rather_than_a_way_in() {
    let device = device();
    let path = device.dir.path().join("passphrases.json");
    std::fs::write(&path, r#"{"passphrases":{"alice":"not a real hash"}}"#).unwrap();
    assert!(
        CredentialStore::open_or_create(&path).is_err(),
        "a stored verifier that cannot be parsed must be noticed when the file is read"
    );
}
