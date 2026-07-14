use std::collections::HashMap;

use hyperion_crypto::{Keystore, Signature, VerifyingKey};

use crate::runtime::InferenceBackend;
use crate::types::{InferenceRequest, ModelDescriptor};

/// The exact fields a real signature is produced/verified over — the same fields the
/// non-cryptographic checksum stand-in this replaces already chose to cover (not `signature`
/// itself, and not anything about the real model weights, which live outside this descriptor
/// entirely), now signed instead of folded into a hash any forger could reproduce without a key.
fn canonical_bytes(descriptor: &ModelDescriptor) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&descriptor.model_id.to_le_bytes());
    bytes.extend_from_slice(&(descriptor.class as u64).to_le_bytes());
    for variant in &descriptor.variants {
        bytes.extend_from_slice(&(variant.footprint_mb as u64).to_le_bytes());
        bytes.extend_from_slice(&(variant.precision as u64).to_le_bytes());
    }
    bytes
}

/// A real Ed25519 signature over `descriptor`'s own canonical bytes (docs/998-roadmap.md
/// M9) — the value a caller populates [`ModelDescriptor::signature`] with before
/// [`crate::runtime::LocalAiRuntime::register_model`], exactly as a real signer would produce a
/// real artifact's real signature.
pub fn sign(descriptor: &ModelDescriptor, keystore: &Keystore) -> Signature {
    keystore.sign(&canonical_bytes(descriptor))
}

/// `true` only if `descriptor.signature` is a genuine, real signature over exactly this
/// descriptor's own canonical bytes, produced by the private key matching `verifying_key` — a
/// tampered field (or a `None` signature) is rejected, not silently accepted. Public: both
/// [`crate::runtime::LocalAiRuntime::register_model`] and `hyperion-security`'s own model-
/// integrity gate (docs/17 T8) need to check the same real signature the same way.
pub fn verify(descriptor: &ModelDescriptor, verifying_key: &VerifyingKey) -> bool {
    match &descriptor.signature {
        Some(signature) => {
            hyperion_crypto::verify(&canonical_bytes(descriptor), signature, verifying_key)
        }
        None => false,
    }
}

/// A deterministic, non-ML stand-in for a real forward pass — see this
/// crate's doc comment. Returns a short, content-derived string so tests
/// can assert on it without pretending it's a real model output. The 500-char
/// cap (previously 200) needs headroom for a base prompt plus a real user's
/// `extra_context` steering text appended after it (`hyperion-agent-runtime`'s
/// `append_extra_context`) — 200 was tight enough that a redo's steering text
/// could be silently cut off before it ever reached this echo.
#[derive(Debug, Default)]
pub struct MockBackend;

impl InferenceBackend for MockBackend {
    fn generate(&self, model_id: u64, request: &InferenceRequest) -> String {
        format!(
            "[mock model {model_id}] echo: {}",
            request.prompt.chars().take(500).collect::<String>()
        )
    }
}

#[derive(Debug, Default)]
pub(crate) struct ModelRegistry {
    descriptors: HashMap<u64, ModelDescriptor>,
}

impl ModelRegistry {
    pub(crate) fn insert(&mut self, descriptor: ModelDescriptor) {
        self.descriptors.insert(descriptor.model_id, descriptor);
    }

    pub(crate) fn get(&self, model_id: u64) -> Option<&ModelDescriptor> {
        self.descriptors.get(&model_id)
    }

    pub(crate) fn by_class(
        &self,
        class: crate::types::ModelClass,
    ) -> impl Iterator<Item = &ModelDescriptor> {
        self.descriptors.values().filter(move |d| d.class == class)
    }
}
