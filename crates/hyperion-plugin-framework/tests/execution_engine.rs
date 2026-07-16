//! docs/998-roadmap.md's Resourceful pillar: `Contribution::ExecutionEngine` is a real, live
//! registration point for "runtimes usable by Capability implementations" (docs/24) — a plugin
//! can install a reusable launcher, have it show up through `PluginRegistry::execution_engine`,
//! disappear again on uninstall/quarantine, and have its own launcher validated the exact same
//! honest way a `Capability`'s own `NativeBinaryDescriptor` already is.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{
    sign, CapabilityGrantRequest, Contribution, ExecutionEngineContribution,
    NativeBinaryDescriptor, Operation, PluginError, PluginManifest, PluginRegistry,
    QuarantineReason, TrustDepth,
};

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

/// The current test binary is a real, already-existing, already-executable file -- a real,
/// honest stand-in for "a real interpreter binary" without needing a purpose-built companion
/// binary for tests that only exercise registration/lookup, not real sandboxed execution (see
/// `hyperion-sdk`'s own tests for the real end-to-end execution proof).
fn real_launcher() -> NativeBinaryDescriptor {
    NativeBinaryDescriptor {
        program: std::env::current_exe().unwrap(),
        args: vec!["--engine-mode".to_string()],
    }
}

fn manifest_with_engine(
    keystore: &Keystore,
    plugin_id: u64,
    launcher: NativeBinaryDescriptor,
) -> PluginManifest {
    let mut manifest = PluginManifest {
        plugin_id,
        publisher: "acme-engines".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::ExecutionEngine(ExecutionEngineContribution {
            engine_id: "python-sandboxed".to_string(),
            launcher,
        })],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::Execute,
            scope: "python-sandboxed".to_string(),
            justification: "the engine's own launcher must be dispatchable".to_string(),
        }],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = Some(sign(&manifest, keystore));
    manifest
}

#[test]
fn installing_an_execution_engine_makes_it_really_discoverable() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    registry
        .install(
            &mut monitor,
            &root,
            manifest_with_engine(&keystore, 1, real_launcher()),
            TrustDepth::D0,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let engine = registry
        .execution_engine("python-sandboxed")
        .expect("a real, installed engine must be found");
    assert_eq!(engine.launcher.program, std::env::current_exe().unwrap());
    assert!(registry.execution_engine("no-such-engine").is_none());
}

#[test]
fn installing_an_engine_with_a_nonexistent_launcher_is_rejected() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let broken_launcher = NativeBinaryDescriptor {
        program: "/definitely/not/a/real/path".into(),
        args: vec![],
    };
    let result = registry.install(
        &mut monitor,
        &root,
        manifest_with_engine(&keystore, 1, broken_launcher),
        TrustDepth::D0,
        true,
        1_000,
        &keystore.verifying_key(),
    );
    assert!(
        matches!(result, Err(PluginError::InvalidNativeBinary(_))),
        "got: {result:?}"
    );
    assert!(registry.execution_engine("python-sandboxed").is_none());
}

#[test]
fn a_network_egress_request_is_never_justified_by_an_execution_engine_alone() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let mut manifest = manifest_with_engine(&keystore, 1, real_launcher());
    manifest.requested_permissions = vec![CapabilityGrantRequest {
        operation: Operation::NetworkEgress,
        scope: "python-sandboxed".to_string(),
        justification: "an execution engine alone can't justify this".to_string(),
    }];
    manifest.signature = Some(sign(&manifest, &keystore));

    let result = registry.install(
        &mut monitor,
        &root,
        manifest,
        TrustDepth::D0,
        true,
        1_000,
        &keystore.verifying_key(),
    );
    assert!(result.is_err());
}

#[test]
fn quarantining_and_uninstalling_removes_it_from_lookup() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let handle = registry
        .install(
            &mut monitor,
            &root,
            manifest_with_engine(&keystore, 1, real_launcher()),
            TrustDepth::D0,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();
    assert!(registry.execution_engine("python-sandboxed").is_some());

    registry
        .quarantine(handle.plugin_id, QuarantineReason::PolicyViolation)
        .unwrap();
    assert!(registry.execution_engine("python-sandboxed").is_none());

    registry
        .uninstall(&mut monitor, &root, handle.plugin_id)
        .unwrap();
    assert!(registry.execution_engine("python-sandboxed").is_none());
}
