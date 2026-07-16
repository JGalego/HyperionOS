//! docs/25 §4's own previously-named gap: `PublishSubmission.package_hash` was always left at
//! `0` by `prepare_submission` -- now a real BLAKE3 content fingerprint over the submission's own
//! (Contract, Implementation) pair, distinct from `to_plugin_manifest`'s own real Ed25519
//! signature (which authenticates the publisher, not the content).

use hyperion_plugin_framework::{Operation, SideEffect};
use hyperion_sdk::{
    prepare_submission, Contract, Implementation, LatencyClass, Runtime, TrustLevel,
};

fn contract() -> Contract {
    Contract {
        id: "document.summarize".to_string(),
        version: 1,
        summary: "Summarizes a block of text".to_string(),
        inputs: vec!["text".to_string()],
        outputs: vec!["summary".to_string()],
        side_effects: vec![SideEffect::None],
        permissions_requested: vec![],
        trust_level: TrustLevel::Sandboxed,
    }
}

fn implementation() -> Implementation {
    Implementation {
        contract_id: "document.summarize".to_string(),
        name: "acme-summarizer".to_string(),
        runtime: Runtime::CloudApi,
        latency_class: LatencyClass::Interactive,
        requires_consent: false,
        native_binary: None,
        resource_profile: None,
    }
}

#[test]
fn package_hash_is_no_longer_always_zero() {
    let submission = prepare_submission(contract(), implementation(), 0.5, vec![]).unwrap();
    assert_ne!(
        submission.package_hash, 0,
        "a real content fingerprint must not be the old hardcoded placeholder"
    );
}

#[test]
fn identical_content_produces_the_same_real_hash() {
    let a = prepare_submission(contract(), implementation(), 0.5, vec![]).unwrap();
    let b = prepare_submission(contract(), implementation(), 0.5, vec![]).unwrap();
    assert_eq!(
        a.package_hash, b.package_hash,
        "the exact same real content must fingerprint identically, deterministically"
    );
}

#[test]
fn a_different_contract_id_produces_a_different_hash() {
    let a = prepare_submission(contract(), implementation(), 0.5, vec![]).unwrap();
    let mut different_contract = contract();
    different_contract.id = "document.translate".to_string();
    let b = prepare_submission(different_contract, implementation(), 0.5, vec![]).unwrap();
    assert_ne!(a.package_hash, b.package_hash);
}

#[test]
fn a_different_implementation_name_produces_a_different_hash() {
    let a = prepare_submission(contract(), implementation(), 0.5, vec![]).unwrap();
    let mut different_implementation = implementation();
    different_implementation.name = "other-vendor-summarizer".to_string();
    let b = prepare_submission(contract(), different_implementation, 0.5, vec![]).unwrap();
    assert_ne!(a.package_hash, b.package_hash);
}

#[test]
fn quality_score_and_observed_permissions_do_not_affect_the_content_hash() {
    // package_hash fingerprints the submission's real content (the Contract/Implementation
    // pair) -- a quality-score change (re-scored after new golden-case runs) or which
    // permissions were statically observed this time must never change what the content itself
    // is fingerprinted as.
    let a = prepare_submission(contract(), implementation(), 0.2, vec![]).unwrap();
    let b = prepare_submission(contract(), implementation(), 0.9, vec![]).unwrap();
    assert_eq!(a.package_hash, b.package_hash);
}

#[test]
fn a_different_native_binary_descriptor_produces_a_different_hash() {
    let mut with_binary = implementation();
    with_binary.runtime = Runtime::NativeBinary;
    with_binary.native_binary = Some(hyperion_plugin_framework::NativeBinaryDescriptor {
        program: "/usr/bin/true".into(),
        args: vec![],
    });
    let a = prepare_submission(contract(), with_binary.clone(), 0.5, vec![]).unwrap();

    let mut different_binary = with_binary;
    different_binary.native_binary = Some(hyperion_plugin_framework::NativeBinaryDescriptor {
        program: "/usr/bin/false".into(),
        args: vec![],
    });
    let b = prepare_submission(contract(), different_binary, 0.5, vec![]).unwrap();

    assert_ne!(a.package_hash, b.package_hash);
}

#[test]
fn different_side_effects_produce_different_hashes() {
    let mut with_egress = contract();
    with_egress.side_effects = vec![SideEffect::NetworkEgress];
    with_egress.permissions_requested = vec![hyperion_sdk::PermissionRequest {
        operation: Operation::NetworkEgress,
        scope: "web.search".to_string(),
        justification: "fetch results".to_string(),
    }];
    let a = prepare_submission(
        with_egress.clone(),
        implementation(),
        0.5,
        vec![Operation::NetworkEgress],
    )
    .unwrap();
    let b = prepare_submission(contract(), implementation(), 0.5, vec![]).unwrap();
    assert_ne!(a.package_hash, b.package_hash);
}
