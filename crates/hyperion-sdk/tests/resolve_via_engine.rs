//! docs/24's "execution engines register runtimes usable by Capability implementations" gap,
//! proven end to end: a plugin's real `Contribution::ExecutionEngine` launcher, a caller's own
//! script path, and `resolve_via_engine`'s resulting `NativeBinaryDescriptor` really install and
//! really run through `PluginRegistry::invoke_native_binary` -- not a simulated resolution.

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::Command;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{
    sign, CapabilityGrantRequest, Contribution, ExecutionEngineContribution,
    NativeBinaryDescriptor, Operation, PluginManifest, PluginRegistry, SideEffect, TrustDepth,
};
use hyperion_sdk::{resolve_via_engine, Contract, Implementation, LatencyClass, Runtime};
use serde_json::json;

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

/// The real, statically-linked companion binary -- see `hyperion-plugin-framework`'s own
/// `src/bin/engine_echo_tool.rs` doc comment for why musl and for the argv contract it honors.
fn engine_launcher_bin() -> PathBuf {
    let target = "x86_64-unknown-linux-musl";
    let status = Command::new("cargo")
        .args([
            "build",
            "--target",
            target,
            "--bin",
            "engine_echo_tool",
            "-p",
            "hyperion-plugin-framework",
        ])
        .status()
        .unwrap_or_else(|e| panic!("run cargo build for the musl engine_echo_tool binary: {e}"));
    assert!(
        status.success(),
        "building the musl engine_echo_tool binary failed"
    );

    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/hyperion-sdk has a workspace root two levels up")
        .to_path_buf();
    workspace_root
        .join("target")
        .join(target)
        .join("debug")
        .join("engine_echo_tool")
}

fn install_engine(registry: &PluginRegistry, launcher: NativeBinaryDescriptor) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let (_dir, keystore) = keystore();

    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-engines".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::ExecutionEngine(ExecutionEngineContribution {
            engine_id: "echo-engine".to_string(),
            launcher,
        })],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::Execute,
            scope: "echo-engine".to_string(),
            justification: "the engine's own launcher must be dispatchable".to_string(),
        }],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = Some(sign(&manifest, &keystore));

    registry
        .install(
            &mut monitor,
            &root,
            manifest,
            TrustDepth::D0,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();
}

#[test]
fn a_capability_resolved_via_an_engine_really_runs_and_receives_the_real_script_path() {
    let registry = PluginRegistry::new();
    install_engine(
        &registry,
        NativeBinaryDescriptor {
            program: engine_launcher_bin(),
            args: vec![],
        },
    );

    let descriptor = resolve_via_engine(
        &registry,
        "echo-engine",
        PathBuf::from("/scripts/greet.py"),
        vec![],
    )
    .expect("a real, installed engine must resolve");
    assert_eq!(descriptor.program, engine_launcher_bin());
    assert_eq!(descriptor.args, vec!["/scripts/greet.py".to_string()]);

    // Install a Capability whose NativeBinaryDescriptor is exactly what resolve_via_engine
    // produced -- the same publish/install path a hand-written NativeBinary already uses.
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let (_dir, keystore) = keystore();
    let contract = Contract {
        id: "greeting.generate".to_string(),
        version: 1,
        summary: "Generates a greeting via a plugin-contributed engine".to_string(),
        inputs: vec!["text".to_string()],
        outputs: vec!["text".to_string()],
        side_effects: vec![SideEffect::None],
        permissions_requested: vec![],
        trust_level: hyperion_sdk::TrustLevel::Sandboxed,
    };
    let implementation = Implementation {
        contract_id: "greeting.generate".to_string(),
        name: "acme-greeter".to_string(),
        runtime: Runtime::NativeBinary,
        latency_class: LatencyClass::Interactive,
        requires_consent: false,
        native_binary: Some(descriptor),
    };
    let submission = hyperion_sdk::prepare_submission(contract, implementation, 0.9, vec![])
        .expect("no undeclared permissions were observed");
    hyperion_sdk::publish(
        &mut monitor,
        &root,
        &registry,
        submission,
        2,
        "acme",
        1,
        false,
        TrustDepth::D0,
        1_000,
        &keystore,
    )
    .expect("publishing a NativeBinary resolved via a real engine must install cleanly");

    let output = registry
        .invoke_native_binary("greeting.generate", json!({"text": "hello"}))
        .expect("the resolved descriptor must really run");
    assert_eq!(output.get("text").and_then(|v| v.as_str()), Some("hello"));
    assert_eq!(
        output.get("received_script").and_then(|v| v.as_str()),
        Some("/scripts/greet.py"),
        "the real script path must have really threaded through the engine's own launcher"
    );
}

#[test]
fn resolving_an_unknown_engine_fails_honestly() {
    let registry = PluginRegistry::new();
    let result = resolve_via_engine(&registry, "no-such-engine", PathBuf::from("x"), vec![]);
    assert!(matches!(
        result,
        Err(hyperion_sdk::SdkError::UnknownExecutionEngine(id)) if id == "no-such-engine"
    ));
}
