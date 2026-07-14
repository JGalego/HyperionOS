use hyperion_crypto::{Keystore, Signature, VerifyingKey};
use serde::Serialize;

use crate::types::{CapabilityManifestEntry, DeviceType};

/// The exact fields a real signature is produced/verified over — everything
/// [`crate::DeviceRegistry::register`] would otherwise trust as given from an
/// advertised manifest (docs/20 §8's device-impersonation defense). Real
/// publisher/manufacturer PKI (a registry of many trusted device-maker keys)
/// does not exist anywhere in this workspace yet — see
/// [`hyperion_crypto`]'s own doc comment on why this crate, like
/// `hyperion-plugin-framework`/`hyperion-update` before it, verifies against
/// one real, trusted device identity rather than inventing an undocumented
/// multi-key trust store.
#[derive(Serialize)]
struct SignedManifestFields<'a> {
    device_type: DeviceType,
    manufacturer: &'a str,
    model: &'a str,
    capability_manifest: &'a [CapabilityManifestEntry],
    owner: u64,
}

fn canonical_bytes(
    device_type: DeviceType,
    manufacturer: &str,
    model: &str,
    capability_manifest: &[CapabilityManifestEntry],
    owner: u64,
) -> Vec<u8> {
    serde_json::to_vec(&SignedManifestFields {
        device_type,
        manufacturer,
        model,
        capability_manifest,
        owner,
    })
    .expect("SignedManifestFields always serializes")
}

/// A real Ed25519 signature over an about-to-be-registered device's own
/// manifest fields (docs/998-roadmap.md M9) — the value a caller
/// passes to [`crate::DeviceRegistry::register`].
#[allow(clippy::too_many_arguments)]
pub fn sign(
    device_type: DeviceType,
    manufacturer: &str,
    model: &str,
    capability_manifest: &[CapabilityManifestEntry],
    owner: u64,
    keystore: &Keystore,
) -> Signature {
    keystore.sign(&canonical_bytes(
        device_type,
        manufacturer,
        model,
        capability_manifest,
        owner,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn verify(
    device_type: DeviceType,
    manufacturer: &str,
    model: &str,
    capability_manifest: &[CapabilityManifestEntry],
    owner: u64,
    signature: &Signature,
    verifying_key: &VerifyingKey,
) -> bool {
    let bytes = canonical_bytes(device_type, manufacturer, model, capability_manifest, owner);
    hyperion_crypto::verify(&bytes, signature, verifying_key)
}
