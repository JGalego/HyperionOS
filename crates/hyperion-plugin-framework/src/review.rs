use hyperion_crypto::{Keystore, Signature, VerifyingKey};

use crate::types::{
    Contribution, Operation, PluginError, PluginManifest, SemanticContract, SideEffect,
};

/// The exact fields a real signature is produced/verified over — the same fields the
/// non-cryptographic-checksum stand-in this replaces already chose to cover, now signed instead
/// of folded into a hash any forger could reproduce without a key. Real publisher-key PKI (a
/// registry of many trusted publishers' public keys, per docs/24's own "verify against
/// publisher's registered key" framing) does not exist anywhere in this workspace yet -- see
/// [`hyperion_crypto`]'s own doc comment on why this crate instead verifies against one real,
/// trusted device identity rather than inventing an undocumented multi-key trust store.
fn canonical_bytes(manifest_without_signature: &PluginManifest) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&manifest_without_signature.plugin_id.to_le_bytes());
    bytes.extend_from_slice(manifest_without_signature.publisher.as_bytes());
    bytes.extend_from_slice(&manifest_without_signature.sdk_version.to_le_bytes());
    for contribution in &manifest_without_signature.contributions {
        match contribution {
            Contribution::Capability(cm) => {
                bytes.extend_from_slice(cm.capability_id.as_bytes());
                bytes.extend_from_slice(&cm.version.to_le_bytes());
            }
            Contribution::Agent(ac) => {
                bytes.extend_from_slice(ac.specialization.as_bytes());
                for capability in &ac.baseline_capabilities {
                    bytes.extend_from_slice(capability.as_bytes());
                }
            }
            Contribution::HardwareSupport(hs) => {
                bytes.extend_from_slice(hs.manufacturer.as_bytes());
                bytes.extend_from_slice(hs.model.as_bytes());
            }
            Contribution::KnowledgeProvider(kp) => {
                bytes.extend_from_slice(kp.topic.as_bytes());
                bytes.extend_from_slice(kp.capability_id.as_bytes());
            }
            Contribution::UiComponent(ui) => {
                bytes.extend_from_slice(ui.capability_ref.as_bytes());
                bytes.extend_from_slice(ui.panel_template.as_bytes());
            }
            Contribution::AutomationWorkflow(wf) => {
                bytes.extend_from_slice(wf.root_predicate.as_bytes());
                for keyword in &wf.trigger_keywords {
                    bytes.extend_from_slice(keyword.as_bytes());
                }
            }
        }
    }
    bytes
}

/// A real Ed25519 signature over `manifest_without_signature`'s own canonical bytes
/// (docs/998-roadmap.md M9) — the value a caller populates [`PluginManifest::signature`]
/// with before [`crate::registry::PluginRegistry::install`].
pub fn sign(manifest_without_signature: &PluginManifest, keystore: &Keystore) -> Signature {
    keystore.sign(&canonical_bytes(manifest_without_signature))
}

fn verify_signature(manifest: &PluginManifest, verifying_key: &VerifyingKey) -> bool {
    let mut unsigned = manifest.clone();
    unsigned.signature = None;
    match &manifest.signature {
        Some(signature) => {
            hyperion_crypto::verify(&canonical_bytes(&unsigned), signature, verifying_key)
        }
        None => false,
    }
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
pub fn validate_manifest(
    manifest: &PluginManifest,
    verifying_key: &VerifyingKey,
) -> Result<(), PluginError> {
    if !verify_signature(manifest, verifying_key) {
        return Err(PluginError::SignatureInvalid);
    }

    for request in &manifest.requested_permissions {
        let justified = manifest.contributions.iter().any(|c| match c {
            Contribution::Capability(cm) => contract_requires(&cm.contract, request.operation),
            // An `Agent` contribution has no `SemanticContract` of its own -- its baseline
            // capabilities are each their own separately-installed `Capability` contribution
            // with its own justification. This variant can only ever justify the two
            // operations an agent's mere existence implies (it must be readable/inspectable and
            // executable to be dispatched); it can never justify `Write`/`NetworkEgress` on its
            // own, so a manifest can't smuggle a data-touching permission in behind an agent.
            Contribution::Agent(_) => {
                matches!(request.operation, Operation::Read | Operation::Execute)
            }
            // A `HardwareSupport` contribution is pure descriptive data (a device driver
            // profile) -- it never executes, writes, or reaches the network on its own, so it
            // can only ever justify `Read`.
            Contribution::HardwareSupport(_) => matches!(request.operation, Operation::Read),
            // A `KnowledgeProvider` contribution is just a (topic -> capability_id) lookup
            // entry -- the capability it points at is a separate, separately-justified
            // `Capability` contribution. This variant alone can only ever justify `Read`.
            Contribution::KnowledgeProvider(_) => matches!(request.operation, Operation::Read),
            // A `UiComponent` contribution is pure descriptive layout/accessibility metadata --
            // it never executes, writes, or reaches the network on its own, so it can only ever
            // justify `Read`.
            Contribution::UiComponent(_) => matches!(request.operation, Operation::Read),
            // An `AutomationWorkflow` contribution is just a declarative task-graph shape --
            // each leaf's predicate maps to its own separately-installed, separately-justified
            // Capability. This variant alone can only ever justify `Read`.
            Contribution::AutomationWorkflow(_) => matches!(request.operation, Operation::Read),
        });
        if !justified {
            return Err(PluginError::PermissionOverreach(request.operation));
        }
    }

    Ok(())
}
