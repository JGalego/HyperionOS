use hyperion_crypto::{Keystore, PublisherRegistry, Signature, VerifyingKey};

use crate::types::{
    Contribution, Operation, PluginError, PluginManifest, SemanticContract, SideEffect,
};

/// The exact bytes a real signature is produced and verified over.
///
/// **What this must cover, and why it is everything.** A signature that commits to less than the
/// manifest *means* is a signature an attacker can reuse. This previously covered a `Capability`
/// contribution's `capability_id` and `version` and nothing else -- so a legitimately signed
/// manifest could have its `native_binary.program` swapped for another executable, its declared
/// side effects rewritten, or its `requested_permissions` widened, and would still verify. On the
/// `install_with_publisher_registry` path, where the point is to trust a third-party publisher's
/// key, that is a real forgery route rather than a theoretical one.
///
/// It now covers every field that decides what installing the manifest will *do*: the program and
/// script that will really execute, the contract (whose inputs carry `hyperion-app`'s own signed
/// owner and durable-storage declaration), the declared side effects the review gate reasons
/// about, every requested permission, and the minimum trust depth.
///
/// **Length-prefixed, not concatenated.** Every variable-length field is written as its length
/// followed by its bytes. Plain concatenation is ambiguous: `("ab", "c")` and `("a", "bc")` produce
/// identical bytes, so two genuinely different manifests could share a signature. That was true of
/// the previous encoding too.
fn canonical_bytes(manifest_without_signature: &PluginManifest) -> Vec<u8> {
    let mut bytes = Vec::new();

    fn push_bytes(out: &mut Vec<u8>, value: &[u8]) {
        out.extend_from_slice(&(value.len() as u64).to_le_bytes());
        out.extend_from_slice(value);
    }
    fn push_str(out: &mut Vec<u8>, value: &str) {
        push_bytes(out, value.as_bytes());
    }
    fn push_contract(out: &mut Vec<u8>, contract: &SemanticContract) {
        push_bytes(out, &(contract.inputs.len() as u64).to_le_bytes());
        for input in &contract.inputs {
            push_str(out, input);
        }
        push_bytes(out, &(contract.outputs.len() as u64).to_le_bytes());
        for output in &contract.outputs {
            push_str(out, output);
        }
        push_bytes(out, &(contract.side_effects.len() as u64).to_le_bytes());
        for side_effect in &contract.side_effects {
            out.push(match side_effect {
                SideEffect::CreatesSemanticObject => 0,
                SideEffect::NetworkEgress => 1,
                SideEffect::None => 2,
            });
        }
    }
    fn push_native_binary(
        out: &mut Vec<u8>,
        native: Option<&crate::types::NativeBinaryDescriptor>,
    ) {
        match native {
            Some(native) => {
                out.push(1);
                push_str(out, &native.program.to_string_lossy());
                push_bytes(out, &(native.args.len() as u64).to_le_bytes());
                for arg in &native.args {
                    push_str(out, arg);
                }
                match &native.script {
                    Some(script) => {
                        out.push(1);
                        push_str(out, &script.to_string_lossy());
                    }
                    None => out.push(0),
                }
            }
            None => out.push(0),
        }
    }

    bytes.extend_from_slice(&manifest_without_signature.plugin_id.to_le_bytes());
    push_str(&mut bytes, &manifest_without_signature.publisher);
    bytes.extend_from_slice(&manifest_without_signature.sdk_version.to_le_bytes());
    bytes.push(match manifest_without_signature.min_trust_depth {
        crate::types::TrustDepth::D0 => 0,
        crate::types::TrustDepth::D1 => 1,
        crate::types::TrustDepth::D2 => 2,
        crate::types::TrustDepth::D3 => 3,
    });

    // Every requested permission. Unsigned, these could be widened after the fact on a manifest
    // whose signature still verified -- which is the whole review gate defeated.
    push_bytes(
        &mut bytes,
        &(manifest_without_signature.requested_permissions.len() as u64).to_le_bytes(),
    );
    for request in &manifest_without_signature.requested_permissions {
        bytes.push(match request.operation {
            Operation::Read => 0,
            Operation::Write => 1,
            Operation::NetworkEgress => 2,
            Operation::Execute => 3,
        });
        push_str(&mut bytes, &request.scope);
        push_str(&mut bytes, &request.justification);
    }

    push_bytes(
        &mut bytes,
        &(manifest_without_signature.contributions.len() as u64).to_le_bytes(),
    );
    for contribution in &manifest_without_signature.contributions {
        // A discriminant per variant, so one contribution's fields can never be read as another's.
        match contribution {
            Contribution::Capability(cm) => {
                bytes.push(0);
                push_str(&mut bytes, &cm.capability_id);
                bytes.extend_from_slice(&cm.version.to_le_bytes());
                bytes.push(match cm.implementation_kind {
                    crate::types::ImplementationKind::LocalSmallModel => 0,
                    crate::types::ImplementationKind::LocalLargeModel => 1,
                    crate::types::ImplementationKind::CloudApi => 2,
                    crate::types::ImplementationKind::NativeBinary => 3,
                });
                bytes.push(match cm.privacy_tier {
                    crate::types::PrivacyTier::Local => 0,
                    crate::types::PrivacyTier::ConsentedCloud => 1,
                });
                push_contract(&mut bytes, &cm.contract);
                push_native_binary(&mut bytes, cm.native_binary.as_ref());
            }
            Contribution::Agent(ac) => {
                bytes.push(1);
                push_str(&mut bytes, &ac.specialization);
                push_bytes(
                    &mut bytes,
                    &(ac.baseline_capabilities.len() as u64).to_le_bytes(),
                );
                for capability in &ac.baseline_capabilities {
                    push_str(&mut bytes, capability);
                }
                push_bytes(
                    &mut bytes,
                    &(ac.requestable_capabilities.len() as u64).to_le_bytes(),
                );
                for capability in &ac.requestable_capabilities {
                    push_str(&mut bytes, capability);
                }
            }
            Contribution::HardwareSupport(hs) => {
                bytes.push(2);
                push_str(&mut bytes, &hs.manufacturer);
                push_str(&mut bytes, &hs.model);
            }
            Contribution::KnowledgeProvider(kp) => {
                bytes.push(3);
                push_str(&mut bytes, &kp.topic);
                push_str(&mut bytes, &kp.capability_id);
            }
            Contribution::UiComponent(ui) => {
                bytes.push(4);
                push_str(&mut bytes, &ui.capability_ref);
                push_str(&mut bytes, &ui.panel_template);
            }
            Contribution::AutomationWorkflow(wf) => {
                bytes.push(5);
                push_str(&mut bytes, &wf.root_predicate);
                push_bytes(
                    &mut bytes,
                    &(wf.trigger_keywords.len() as u64).to_le_bytes(),
                );
                for keyword in &wf.trigger_keywords {
                    push_str(&mut bytes, keyword);
                }
            }
            Contribution::MemoryProvider(mp) => {
                bytes.push(6);
                bytes.push(mp.tier as u8);
                push_str(&mut bytes, &mp.entity_key);
                push_str(&mut bytes, &mp.capability_id);
            }
            Contribution::ExecutionEngine(ee) => {
                bytes.push(7);
                push_str(&mut bytes, &ee.engine_id);
                // The launcher every script published through this engine will run through.
                push_native_binary(&mut bytes, Some(&ee.launcher));
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
    check_permission_overreach(manifest)
}

/// As [`validate_manifest`], but resolving `manifest.publisher`'s real, trusted key from
/// `publishers` instead of taking one caller-supplied key on faith — docs/24's own "verify
/// against publisher's registered key" framing, made real. A publisher `install_with_
/// publisher_registry`/`update_with_publisher_registry` has never registered a key for is a real,
/// honest [`PluginError::UnknownPublisher`], never a silent fall-through to some other trust.
pub fn validate_manifest_against_registry(
    manifest: &PluginManifest,
    publishers: &PublisherRegistry,
) -> Result<(), PluginError> {
    let verifying_key = publishers
        .verifying_key_for(&manifest.publisher)
        .ok_or_else(|| PluginError::UnknownPublisher(manifest.publisher.clone()))?;
    validate_manifest(manifest, &verifying_key)
}

fn check_permission_overreach(manifest: &PluginManifest) -> Result<(), PluginError> {
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
            // A `MemoryProvider` contribution is just a (tier, entity_key) -> capability_id
            // lookup entry -- the capability it points at is a separate, separately-justified
            // `Capability` contribution. This variant alone can only ever justify `Read`.
            Contribution::MemoryProvider(_) => matches!(request.operation, Operation::Read),
            // An `ExecutionEngine` contribution's own launcher really executes whatever script a
            // caller later resolves through it -- the same "must be executable to be dispatched"
            // reasoning `Agent` already gets -- but it never writes data or reaches the network
            // on its own; any capability that ends up running through it is its own separate,
            // separately-justified `Capability` contribution.
            Contribution::ExecutionEngine(_) => {
                matches!(request.operation, Operation::Read | Operation::Execute)
            }
        });
        if !justified {
            return Err(PluginError::PermissionOverreach(request.operation));
        }
    }

    Ok(())
}
