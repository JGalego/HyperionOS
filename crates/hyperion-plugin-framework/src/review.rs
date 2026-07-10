use crate::types::{
    Contribution, Operation, PluginError, PluginManifest, SemanticContract, SideEffect,
};

/// A deterministic stand-in for a real publisher-key signature — the same
/// non-cryptographic-checksum pattern this workspace already uses in
/// `hyperion-ai-runtime::checksum` and `hyperion-security`'s model
/// integrity check. Exposed so a caller populating
/// [`PluginManifest::signature`] before install has a way to compute the
/// value that will verify.
pub fn signature(manifest_without_signature: &PluginManifest) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut mix = |value: u64| {
        hash ^= value;
        hash = hash.wrapping_mul(0x100000001b3);
    };
    mix(manifest_without_signature.plugin_id);
    for byte in manifest_without_signature.publisher.bytes() {
        mix(byte as u64);
    }
    mix(manifest_without_signature.sdk_version as u64);
    for contribution in &manifest_without_signature.contributions {
        let Contribution::Capability(cm) = contribution;
        for byte in cm.capability_id.bytes() {
            mix(byte as u64);
        }
        mix(cm.version as u64);
    }
    hash
}

/// docs/24 §5's over-request check: a requested permission must be
/// justified by a declared side effect somewhere in the manifest's
/// contributions — a Capability declaring `side_effects: [None]` cannot
/// request `NetworkEgress`, and this is rejected pre-consent, never
/// surfaced as a choice the user could accidentally approve.
pub(crate) fn contract_requires(contract: &SemanticContract, op: Operation) -> bool {
    match op {
        Operation::NetworkEgress => contract.side_effects.contains(&SideEffect::NetworkEgress),
        Operation::Write => {
            contract
                .side_effects
                .contains(&SideEffect::CreatesSemanticObject)
                || contract.side_effects.contains(&SideEffect::NetworkEgress)
        }
        Operation::Read | Operation::Execute => true,
    }
}

/// docs/24 §5's review-gate steps that don't require a live
/// `CapabilityMonitor`: signature verification and the per-permission
/// over-request check. Trust-depth and consent are checked separately by
/// [`crate::registry::PluginRegistry::install`] since they need caller-
/// supplied context (the installing environment's available depth, and
/// the consent decision itself) this pure function doesn't have.
pub fn validate_manifest(manifest: &PluginManifest) -> Result<(), PluginError> {
    let mut unsigned = manifest.clone();
    unsigned.signature = 0;
    if signature(&unsigned) != manifest.signature {
        return Err(PluginError::SignatureInvalid);
    }

    for request in &manifest.requested_permissions {
        let justified = manifest.contributions.iter().any(|c| match c {
            Contribution::Capability(cm) => contract_requires(&cm.contract, request.operation),
        });
        if !justified {
            return Err(PluginError::PermissionOverreach(request.operation));
        }
    }

    Ok(())
}
