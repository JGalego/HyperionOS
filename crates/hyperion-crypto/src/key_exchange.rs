//! Real X25519 Diffie-Hellman key agreement -- closes `hyperion-federation`'s own named
//! deferral: [`sync_envelope`](crate::sync_envelope)'s original scope assumed sealer and opener
//! already shared one [`crate::Keystore`], "not yet a real key-exchange between genuinely
//! separate, independently-keyed devices." [`Keystore::x25519_public`]/[`diffie_hellman`] are the
//! real, minimal addition that makes that possible: two devices exchange their real public X25519
//! keys (over whatever channel already carries a `SyncEnvelope`) and each independently derives
//! the identical real shared secret -- the defining property of Diffie-Hellman, verified directly
//! in this module's own tests.
//!
//! The X25519 static secret is deterministically derived from the same per-device Ed25519 signing
//! identity via [`crate::Keystore::derive_key`] (the same domain-separation convention
//! [`crate::secret_store`]/[`crate::sync_envelope`] already established) rather than a second,
//! independently-generated and separately-persisted keypair -- one real root identity per device
//! remains the one thing a caller has to manage, matching this crate's own doc comment ("one real
//! device identity... every real signature in this workspace signs with the *same* device key").

use x25519_dalek::{PublicKey, StaticSecret};

use crate::Keystore;

const KEY_DERIVATION_CONTEXT: &str = "hyperion.x25519-static-secret.v1";

impl Keystore {
    /// This device's real, deterministic X25519 static secret, derived from its own Ed25519
    /// signing identity -- never persisted separately, never leaves this process.
    fn x25519_secret(&self) -> StaticSecret {
        StaticSecret::from(self.derive_key(KEY_DERIVATION_CONTEXT))
    }

    /// This device's real X25519 public key -- what a peer needs to derive the same shared
    /// secret via [`diffie_hellman`]. Safe to publish; unlike [`Self::sign`]'s `VerifyingKey`,
    /// this is a distinct keypair used only for key agreement, never for signing.
    pub fn x25519_public(&self) -> PublicKey {
        PublicKey::from(&self.x25519_secret())
    }
}

/// Real X25519 Diffie-Hellman: `diffie_hellman(a, b.x25519_public()) ==
/// diffie_hellman(b, a.x25519_public())` for any two real, independent [`Keystore`]s `a`/`b` --
/// the actual shared-secret property this function exists to provide, proven directly in this
/// module's own tests. The raw ECDH output is never used as an AEAD key directly; a caller (see
/// [`crate::sync_envelope::seal_for_peer`]/[`crate::sync_envelope::open_from_peer`]) runs it
/// through a further, purpose-specific key derivation first.
pub fn diffie_hellman(my_keystore: &Keystore, their_public: &PublicKey) -> [u8; 32] {
    my_keystore
        .x25519_secret()
        .diffie_hellman(their_public)
        .to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_independent_keystores_derive_the_identical_real_shared_secret() {
        let alice = Keystore::ephemeral();
        let bob = Keystore::ephemeral();

        let alice_side = diffie_hellman(&alice, &bob.x25519_public());
        let bob_side = diffie_hellman(&bob, &alice.x25519_public());

        assert_eq!(
            alice_side, bob_side,
            "a real X25519 exchange must produce the identical shared secret on both sides"
        );
    }

    #[test]
    fn two_different_keystore_pairs_never_agree_on_the_same_secret() {
        let alice = Keystore::ephemeral();
        let bob = Keystore::ephemeral();
        let mallory = Keystore::ephemeral();

        let alice_bob = diffie_hellman(&alice, &bob.x25519_public());
        let alice_mallory = diffie_hellman(&alice, &mallory.x25519_public());

        assert_ne!(
            alice_bob, alice_mallory,
            "a shared secret must be specific to the actual pair of devices involved"
        );
    }

    #[test]
    fn x25519_public_is_deterministic_and_stable_for_the_same_real_identity() {
        let alice = Keystore::ephemeral();
        assert_eq!(
            alice.x25519_public().as_bytes(),
            alice.x25519_public().as_bytes(),
            "the same device identity must always derive the same real X25519 public key"
        );
    }
}
