use hyperion_crypto::{hash, verify, Keystore};

#[test]
fn a_fresh_keystore_generates_a_real_key_and_persists_it_across_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let key_path = dir.path().join("device.key");
    assert!(!key_path.exists());

    let first = Keystore::open_or_create(&key_path).expect("generate a real key on first open");
    assert!(key_path.exists(), "the real key must be persisted to disk");
    let vk1 = first.verifying_key();

    let second = Keystore::open_or_create(&key_path).expect("reload the same real key");
    let vk2 = second.verifying_key();
    assert_eq!(
        vk1, vk2,
        "reopening an existing keystore must load the same real key, not generate a new one"
    );
}

#[cfg(unix)]
#[test]
fn a_freshly_created_key_file_is_owner_only_readable() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let key_path = dir.path().join("device.key");
    Keystore::open_or_create(&key_path).unwrap();

    let mode = std::fs::metadata(&key_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "a real private key file must not be group/world readable"
    );
}

#[test]
fn a_real_signature_verifies_only_against_the_exact_signed_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();

    let message = b"a real plugin manifest's real canonical bytes";
    let signature = keystore.sign(message);

    assert!(
        verify(message, &signature, &keystore.verifying_key()),
        "a real signature must verify against the exact bytes it was produced over"
    );
    assert!(
        !verify(b"a tampered message", &signature, &keystore.verifying_key()),
        "a real signature must NOT verify once the signed content has changed -- this is the \
         whole property a non-cryptographic checksum lacked"
    );
}

#[test]
fn a_signature_from_a_different_keystore_does_not_verify() {
    let dir = tempfile::tempdir().unwrap();
    let real_signer = Keystore::open_or_create(&dir.path().join("real.key")).unwrap();
    let forger = Keystore::open_or_create(&dir.path().join("forger.key")).unwrap();

    let message = b"only the real device key may attest this";
    let forged_signature = forger.sign(message);

    assert!(
        !verify(message, &forged_signature, &real_signer.verifying_key()),
        "a signature from a forger's own real (but different) keypair must not verify against \
         the real signer's public key -- unforgeability without the private key is the entire \
         point of this milestone"
    );
}

#[test]
fn the_same_bytes_always_hash_to_the_same_value_and_different_bytes_do_not() {
    let a = hash(b"identical content");
    let b = hash(b"identical content");
    let c = hash(b"different content");
    assert_eq!(a, b, "hashing is deterministic");
    assert_ne!(a, c, "different content must not collide in practice");
}
