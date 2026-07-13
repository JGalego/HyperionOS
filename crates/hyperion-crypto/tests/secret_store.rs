use hyperion_crypto::{Keystore, SecretStore, SecretStoreError};

#[test]
fn a_stored_secret_survives_a_real_encrypt_decrypt_round_trip_across_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let device_key = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    let store_path = dir.path().join("secrets.enc");

    let mut first = SecretStore::open_or_create(&store_path, &device_key)
        .expect("create a fresh real secret store");
    first
        .set("openai", "sk-real-test-key")
        .expect("encrypt and persist a real secret");
    assert!(
        store_path.exists(),
        "the real store must be persisted to disk"
    );

    let second = SecretStore::open_or_create(&store_path, &device_key)
        .expect("reopen and really decrypt the same store");
    assert_eq!(
        second.get("openai"),
        Some("sk-real-test-key"),
        "a reopened store must really decrypt to the same secret that was set, not garbage or \
         nothing"
    );
}

#[test]
fn the_on_disk_file_never_contains_the_real_secret_as_plaintext() {
    let dir = tempfile::tempdir().unwrap();
    let device_key = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    let store_path = dir.path().join("secrets.enc");

    let mut store = SecretStore::open_or_create(&store_path, &device_key).unwrap();
    store.set("anthropic", "sk-ant-super-secret-value").unwrap();

    let raw_bytes = std::fs::read(&store_path).unwrap();
    let raw_text = String::from_utf8_lossy(&raw_bytes);
    assert!(
        !raw_text.contains("sk-ant-super-secret-value"),
        "the real secret must never appear as plaintext in the encrypted-at-rest file"
    );
}

#[test]
fn a_different_device_key_fails_closed_instead_of_returning_garbage() {
    let dir = tempfile::tempdir().unwrap();
    let real_device_key = Keystore::open_or_create(&dir.path().join("real.key")).unwrap();
    let store_path = dir.path().join("secrets.enc");

    let mut store = SecretStore::open_or_create(&store_path, &real_device_key).unwrap();
    store.set("gemini", "a-real-key").unwrap();

    let wrong_device_key = Keystore::open_or_create(&dir.path().join("wrong.key")).unwrap();
    let err = SecretStore::open_or_create(&store_path, &wrong_device_key)
        .err()
        .expect("opening with the wrong device-derived key must fail, not silently succeed");

    assert!(
        matches!(err, SecretStoreError::DecryptionFailed(_)),
        "opening a real store with the wrong device-derived key must fail closed via a real \
         authentication-tag mismatch, not silently succeed with wrong/garbage secrets, got: \
         {err}"
    );
}

#[test]
fn a_fresh_store_with_no_file_yet_starts_empty_and_creates_one_on_first_set() {
    let dir = tempfile::tempdir().unwrap();
    let device_key = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    let store_path = dir.path().join("secrets.enc");
    assert!(!store_path.exists());

    let mut store = SecretStore::open_or_create(&store_path, &device_key).unwrap();
    assert_eq!(store.get("openai"), None, "an unset provider has no secret");
    assert_eq!(
        store.providers().count(),
        0,
        "a fresh store has no providers yet"
    );

    store.set("openai", "sk-real").unwrap();
    assert!(
        store_path.exists(),
        "the first real set() must persist a real file"
    );
    assert_eq!(store.providers().collect::<Vec<_>>(), vec!["openai"]);
}
