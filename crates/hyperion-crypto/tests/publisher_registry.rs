//! docs/24's own named "multi-party / publisher trust stores" gap: a real registry of trusted
//! publisher public keys, closed for a caller that wants real per-publisher trust.

use hyperion_crypto::{Keystore, PublisherRegistry};

#[test]
fn an_unregistered_publisher_has_no_trusted_key() {
    let registry = PublisherRegistry::new();
    assert!(registry.verifying_key_for("acme-plugins").is_none());
}

#[test]
fn a_registered_publishers_real_key_is_looked_up_by_its_own_id() {
    let mut registry = PublisherRegistry::new();
    let acme = Keystore::ephemeral();
    let globex = Keystore::ephemeral();

    registry.register("acme-plugins", acme.verifying_key());
    registry.register("globex-plugins", globex.verifying_key());

    assert_eq!(
        registry.verifying_key_for("acme-plugins"),
        Some(acme.verifying_key())
    );
    assert_eq!(
        registry.verifying_key_for("globex-plugins"),
        Some(globex.verifying_key())
    );
    assert_ne!(
        registry.verifying_key_for("acme-plugins"),
        registry.verifying_key_for("globex-plugins")
    );
}

#[test]
fn re_registering_the_same_publisher_id_really_rotates_its_key() {
    let mut registry = PublisherRegistry::new();
    let old_key = Keystore::ephemeral();
    let new_key = Keystore::ephemeral();

    registry.register("acme-plugins", old_key.verifying_key());
    assert_eq!(
        registry.verifying_key_for("acme-plugins"),
        Some(old_key.verifying_key())
    );

    registry.register("acme-plugins", new_key.verifying_key());
    assert_eq!(
        registry.verifying_key_for("acme-plugins"),
        Some(new_key.verifying_key()),
        "re-registering the same publisher id must really replace its trusted key"
    );
}
