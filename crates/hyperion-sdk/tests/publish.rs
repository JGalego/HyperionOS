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
        // Not `Runtime::NativeBinary`: `PluginRegistry::install` really validates a `NativeBinary`
        // contribution needs a real, existing, executable `NativeBinaryDescriptor` -- these tests
        // exercise the generic publish/harness/registry workflow, not native-binary execution
        // specifically, so any non-`NativeBinary` runtime is the right, honest fixture choice. See
        // `a_native_binary_submission_installs_as_a_real_runnable_capability` below for the real
        // `NativeBinary` + `native_binary: Some(..)` path, end to end.
        runtime: Runtime::CloudApi,
        latency_class: LatencyClass::Interactive,
        requires_consent: false,
        native_binary: None,
        resource_profile: None,
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

/// docs/998-roadmap.md's "tool creation" slice, proven end to end at this crate's own real entry
/// point: naming a real, existing, executable program as a `Runtime::NativeBinary`
/// `Implementation` and publishing it doesn't just land a labeled placeholder in the registry --
/// it's genuinely *runnable* the moment `publish` returns, through the exact same real, sandboxed
/// `hyperion-plugin-framework::PluginRegistry::invoke_native_binary` path Slice 1 built. Linux-only,
/// matching `hyperion-trust-boundary`'s own gating.
#[cfg(target_os = "linux")]
#[test]
fn a_native_binary_submission_installs_as_a_real_runnable_capability() {
    use hyperion_plugin_framework::NativeBinaryDescriptor;

    fn uppercase_tool_bin() -> std::path::PathBuf {
        let target = "x86_64-unknown-linux-musl";
        let status = std::process::Command::new("cargo")
            .args([
                "build",
                "--target",
                target,
                "--bin",
                "uppercase_tool",
                "-p",
                "hyperion-plugin-framework",
            ])
            .status()
            .expect("run cargo build for the musl uppercase_tool binary");
        assert!(
            status.success(),
            "building the musl uppercase_tool binary failed"
        );

        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("crates/hyperion-sdk has a workspace root two levels up")
            .to_path_buf();
        workspace_root
            .join("target")
            .join(target)
            .join("debug")
            .join("uppercase_tool")
    }

    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let mut contract = contract_without_permissions();
    contract.id = "text.uppercase".to_string();
    contract.permissions_requested = vec![PermissionRequest {
        operation: Operation::Execute,
        scope: "text.uppercase".to_string(),
        justification: "run the real sandboxed tool".to_string(),
    }];

    let new_tool = Implementation {
        contract_id: "text.uppercase".to_string(),
        name: "acme-uppercase".to_string(),
        runtime: Runtime::NativeBinary,
        latency_class: LatencyClass::Interactive,
        requires_consent: false,
        native_binary: Some(NativeBinaryDescriptor {
            program: uppercase_tool_bin(),
            args: vec![],
        }),
        resource_profile: None,
    };

    let submission = prepare_submission(contract, new_tool, 0.5, vec![]).unwrap();
    publish(
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
    )
    .unwrap();

    let result = registry
        .invoke_native_binary("text.uppercase", serde_json::json!({"text": "new tool"}))
        .expect("a just-published NativeBinary capability must really run");
    assert_eq!(
        result.get("text").and_then(|v| v.as_str()),
        Some("NEW TOOL"),
        "got: {result:?}"
    );
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
