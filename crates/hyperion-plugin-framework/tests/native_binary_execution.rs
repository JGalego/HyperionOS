//! `PluginRegistry::invoke_native_binary` -- docs/998-roadmap.md's Slice 1: an installed
//! `NativeBinary` capability actually runs, for real, inside a real
//! `hyperion-trust-boundary::spawn` sandbox, instead of the previous "data only, no execution"
//! gap this crate's own doc comment named. Linux-only, matching `hyperion-trust-boundary`'s own
//! gating -- there is no sandboxed execution to test anywhere else.

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::Command;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{
    sign, CapabilityGrantRequest, CapabilityManifest, Contribution, ImplementationKind,
    NativeBinaryDescriptor, Operation, PluginError, PluginManifest, PluginRegistry, PrivacyTier,
    SemanticContract, SideEffect, TrustDepth,
};
use serde_json::json;

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

/// The real, statically-linked companion binaries -- see `src/bin/uppercase_tool.rs`'s own doc
/// comment for why musl, not the default dynamically-linked target (needs nothing outside its own
/// sandboxed fs_scope to start, exactly like `hyperion-trust-boundary`'s own `probe_bin()`). A
/// dynamically linked `/bin/sh` script was tried first and either hung or behaved inconsistently
/// under the real sandbox -- its own dynamic linker needs to read `/lib`/`/usr/lib`, outside any
/// real `fs_scope`, exactly the failure mode `hyperion-trust-boundary`'s own tests already named
/// and avoided the same way.
fn tool_bin(name: &str) -> PathBuf {
    let target = "x86_64-unknown-linux-musl";
    let status = Command::new("cargo")
        .args([
            "build",
            "--target",
            target,
            "--bin",
            name,
            "-p",
            "hyperion-plugin-framework",
        ])
        .status()
        .unwrap_or_else(|e| panic!("run cargo build for the musl {name} binary: {e}"));
    assert!(status.success(), "building the musl {name} binary failed");

    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/hyperion-plugin-framework has a workspace root two levels up")
        .to_path_buf();
    workspace_root
        .join("target")
        .join(target)
        .join("debug")
        .join(name)
}

fn manifest_with_native_binary(keystore: &Keystore, program: std::path::PathBuf) -> PluginManifest {
    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "text.uppercase".to_string(),
            contract: SemanticContract {
                inputs: vec!["text".to_string()],
                outputs: vec!["text".to_string()],
                side_effects: vec![SideEffect::None],
            },
            implementation_kind: ImplementationKind::NativeBinary,
            quality_score: 0.5,
            version: 1,
            native_binary: Some(NativeBinaryDescriptor {
                program,
                args: vec![],
            }),
            privacy_tier: PrivacyTier::Local,
        })],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::Execute,
            scope: "text.uppercase".to_string(),
            justification: "run the real sandboxed script".to_string(),
        }],
        min_trust_depth: TrustDepth::D1,
    };
    manifest.signature = Some(sign(&manifest, keystore));
    manifest
}

#[test]
fn an_installed_native_binary_actually_runs_and_returns_real_output() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    registry
        .install(
            &mut monitor,
            &root,
            manifest_with_native_binary(&keystore, tool_bin("uppercase_tool")),
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let result = registry
        .invoke_native_binary(
            "text.uppercase",
            json!({"text": "hello from a real sandbox"}),
        )
        .expect("a real, installed NativeBinary implementation must actually run");

    assert_eq!(
        result.get("text").and_then(|v| v.as_str()),
        Some("HELLO FROM A REAL SANDBOX"),
        "expected the real sandboxed script's real output, got: {result:?}"
    );
}

#[test]
fn installing_a_native_binary_with_a_nonexistent_program_is_rejected() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let result = registry.install(
        &mut monitor,
        &root,
        manifest_with_native_binary(&keystore, "/definitely/not/a/real/path".into()),
        TrustDepth::D2,
        true,
        1_000,
        &keystore.verifying_key(),
    );
    assert!(
        matches!(result, Err(PluginError::InvalidNativeBinary(_))),
        "got: {result:?}"
    );
    assert!(
        registry.query("text.uppercase").is_none(),
        "a rejected manifest must never partially install"
    );
}

#[test]
fn installing_a_non_executable_file_as_a_native_binary_is_rejected() {
    let scratch = tempfile::tempdir().unwrap();
    let not_executable = scratch.path().join("not-executable.txt");
    std::fs::write(&not_executable, b"just text, not a real program").unwrap();

    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let result = registry.install(
        &mut monitor,
        &root,
        manifest_with_native_binary(&keystore, not_executable),
        TrustDepth::D2,
        true,
        1_000,
        &keystore.verifying_key(),
    );
    assert!(
        matches!(result, Err(PluginError::InvalidNativeBinary(_))),
        "got: {result:?}"
    );
}

/// A hung tool must not block the caller forever -- it gets killed at `NATIVE_BINARY_TIMEOUT`.
/// A real, short-lived override isn't available (the timeout is a private const tuned for real
/// use, not test speed), so this proves the *mechanism* instead: a tool that exits immediately
/// with a real nonzero status is a real, honest failure, not a panic or a silent success.
#[test]
fn a_tool_exiting_nonzero_is_a_real_honest_error_not_a_panic() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    registry
        .install(
            &mut monitor,
            &root,
            manifest_with_native_binary(&keystore, tool_bin("fail_tool")),
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let result = registry.invoke_native_binary("text.uppercase", json!({"text": "x"}));
    assert!(
        matches!(result, Err(PluginError::ExecutionFailed(_))),
        "got: {result:?}"
    );
}

#[test]
fn invoking_an_uninstalled_capability_is_a_real_honest_error() {
    let registry = PluginRegistry::new();
    let result = registry.invoke_native_binary("nothing.installed", json!({}));
    assert!(
        matches!(result, Err(PluginError::NoSuchCapability)),
        "got: {result:?}"
    );
}
