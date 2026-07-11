//! docs/25 §4's publish workflow: static permission analysis fails the
//! build before review, sensitive permissions force human review, and a
//! published (Contract, Implementation) pair lands in the real
//! `hyperion-plugin-framework` registry as one more competing candidate.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{Operation, PluginRegistry, SideEffect, TrustDepth};
use hyperion_sdk::{
    prepare_submission, publish, Contract, Implementation, LatencyClass, PermissionRequest,
    ReviewStatus, Runtime, SdkError, TrustLevel,
};

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

fn contract_without_permissions() -> Contract {
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
        runtime: Runtime::NativeBinary,
        latency_class: LatencyClass::Interactive,
        requires_consent: false,
    }
}

#[test]
fn an_implementation_that_statically_observes_an_undeclared_permission_fails_the_build() {
    let result = prepare_submission(
        contract_without_permissions(),
        implementation(),
        0.5,
        vec![Operation::NetworkEgress],
    );
    assert!(matches!(
        result,
        Err(SdkError::UndeclaredPermissionObserved)
    ));
}

#[test]
fn a_contract_with_no_sensitive_permissions_is_auto_approved() {
    let submission = prepare_submission(
        contract_without_permissions(),
        implementation(),
        0.5,
        vec![],
    )
    .unwrap();
    assert_eq!(submission.review_status, ReviewStatus::AutoApproved);
}

#[test]
fn a_contract_requesting_network_egress_requires_human_review() {
    let mut contract = contract_without_permissions();
    contract.side_effects = vec![SideEffect::NetworkEgress];
    contract.permissions_requested = vec![PermissionRequest {
        operation: Operation::NetworkEgress,
        scope: "web.search".to_string(),
        justification: "fetch results".to_string(),
    }];

    let submission = prepare_submission(
        contract,
        implementation(),
        0.5,
        vec![Operation::NetworkEgress],
    )
    .unwrap();
    assert_eq!(submission.review_status, ReviewStatus::PendingHumanReview);
}

#[test]
fn publishing_without_the_required_human_approval_is_rejected() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let mut contract = contract_without_permissions();
    contract.side_effects = vec![SideEffect::NetworkEgress];
    contract.permissions_requested = vec![PermissionRequest {
        operation: Operation::NetworkEgress,
        scope: "web.search".to_string(),
        justification: "fetch results".to_string(),
    }];
    let submission = prepare_submission(
        contract,
        implementation(),
        0.5,
        vec![Operation::NetworkEgress],
    )
    .unwrap();

    let result = publish(
        &mut monitor,
        &root,
        &registry,
        submission,
        1,
        "acme-plugins",
        1,
        false,
        TrustDepth::D2,
        1_000,
        &keystore,
    );
    assert!(matches!(result, Err(SdkError::SubmissionRejected)));
    assert!(registry.query("document.summarize").is_none());
}

#[test]
fn a_published_capability_lands_in_the_real_registry_as_a_candidate() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let submission = prepare_submission(
        contract_without_permissions(),
        implementation(),
        0.5,
        vec![],
    )
    .unwrap();
    let handle = publish(
        &mut monitor,
        &root,
        &registry,
        submission,
        1,
        "acme-plugins",
        1,
        false,
        TrustDepth::D0,
        1_000,
        &keystore,
    )
    .unwrap();

    let entry = registry.query("document.summarize").unwrap();
    assert_eq!(entry.owning_plugins, vec![handle.plugin_id]);
}

#[test]
fn a_second_independently_published_capability_competes_as_a_second_candidate() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let first = prepare_submission(
        contract_without_permissions(),
        implementation(),
        0.5,
        vec![],
    )
    .unwrap();
    publish(
        &mut monitor,
        &root,
        &registry,
        first,
        1,
        "acme-plugins",
        1,
        false,
        TrustDepth::D0,
        1_000,
        &keystore,
    )
    .unwrap();

    let mut second_impl = implementation();
    second_impl.name = "globex-summarizer".to_string();
    let second =
        prepare_submission(contract_without_permissions(), second_impl, 0.5, vec![]).unwrap();
    publish(
        &mut monitor,
        &root,
        &registry,
        second,
        2,
        "globex-plugins",
        1,
        false,
        TrustDepth::D0,
        1_001,
        &keystore,
    )
    .unwrap();

    let entry = registry.query("document.summarize").unwrap();
    assert_eq!(entry.implementations.len(), 2, "a third-party Capability and a first-party equivalent must both be visible to the Model Router as competing candidates");
}
