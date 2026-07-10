use std::collections::HashMap;

use crate::runtime::InferenceBackend;
use crate::types::{InferenceRequest, ModelDescriptor};

/// A non-cryptographic checksum over a model's variant footprints — a
/// stand-in for the real signature docs/22 §Security Considerations
/// requires; see this crate's doc comment. Public because a caller
/// populating [`ModelDescriptor::checksum`] before
/// [`crate::LocalAiRuntime::register_model`] needs a way to compute the
/// value that will verify, exactly as a real signer would need the
/// artifact's real signature.
pub fn checksum(descriptor: &ModelDescriptor) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut mix = |value: u64| {
        hash ^= value;
        hash = hash.wrapping_mul(0x100000001b3);
    };
    mix(descriptor.model_id);
    mix(descriptor.class as u64);
    for variant in &descriptor.variants {
        mix(variant.footprint_mb as u64);
        mix(variant.precision as u64);
    }
    hash
}

/// A deterministic, non-ML stand-in for a real forward pass — see this
/// crate's doc comment. Returns a short, content-derived string so tests
/// can assert on it without pretending it's a real model output.
#[derive(Debug, Default)]
pub struct MockBackend;

impl InferenceBackend for MockBackend {
    fn generate(&self, model_id: u64, request: &InferenceRequest) -> String {
        format!(
            "[mock model {model_id}] echo: {}",
            request.prompt.chars().take(200).collect::<String>()
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
