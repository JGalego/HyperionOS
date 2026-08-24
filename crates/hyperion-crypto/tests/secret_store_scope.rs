//! Per-scope secret separation (docs/998-roadmap.md §0, Decision 2).

use hyperion_crypto::{Keystore, SecretStore};

#[test]
fn two_scopes_cannot_read_each_others_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();

    let alice_path = dir.path().join("alice.enc");
    {
        let mut alice =
            SecretStore::open_or_create_scoped(&alice_path, &keystore, Some("user.alice")).unwrap();
        alice.set("openai", "sk-alice").unwrap();
    }

    // The same device key and the same file, a different scope: the authentication tag really
    // rejects it rather than returning anything. One person's turn cannot spend another's credit.
    let as_bob = SecretStore::open_or_create_scoped(&alice_path, &keystore, Some("user.bob"));
    assert!(
        as_bob.is_err(),
        "a different scope must not decrypt another's store"
    );

    let as_alice =
        SecretStore::open_or_create_scoped(&alice_path, &keystore, Some("user.alice")).unwrap();
    assert_eq!(as_alice.get("openai"), Some("sk-alice"));
}

#[test]
fn an_unscoped_store_is_byte_for_byte_what_it_always_was() {
    // Every store written before scopes existed has to stay readable, so `open_or_create` and
    // `open_or_create_scoped(.., None)` must derive the same key.
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    let path = dir.path().join("legacy.enc");

    {
        let mut legacy = SecretStore::open_or_create(&path, &keystore).unwrap();
        legacy.set("openai", "sk-legacy").unwrap();
    }
    let reopened = SecretStore::open_or_create_scoped(&path, &keystore, None).unwrap();
    assert_eq!(reopened.get("openai"), Some("sk-legacy"));
}
