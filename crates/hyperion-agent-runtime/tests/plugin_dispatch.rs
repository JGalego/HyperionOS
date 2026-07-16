//! docs/998-roadmap.md's Slice 1, proven at this crate's own real entry point:
//! `AgentRuntime::invoke`, given a `capability_ref` it doesn't otherwise recognize, dispatches to
//! a real, installed `PluginRegistry` `NativeBinary` implementation -- a real sandboxed subprocess,
//! not `stubs::dispatch`'s catch-all echo. Linux-only, matching `hyperion-trust-boundary`'s own
//! gating (real sandboxed execution doesn't exist anywhere else).

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use hyperion_agent_runtime::{AgentManifest, AgentRuntime, InvokeOutcome, TrustTier};
use hyperion_ai_runtime::{LocalAiRuntime, MockBackend};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{
    sign, CapabilityGrantRequest, CapabilityManifest, Contribution, ImplementationKind,
    NativeBinaryDescriptor, Operation, PluginManifest, PluginRegistry, PrivacyTier,
    SemanticContract, SideEffect, TrustDepth,
};
use serde_json::json;

/// The exact same real, statically-linked companion binary
/// `hyperion-plugin-framework/tests/native_binary_execution.rs` already builds and proves --
/// reused here rather than duplicated, to prove the *dispatch wiring*, not re-prove the sandbox
/// itself.
fn uppercase_tool_bin() -> PathBuf {
    let target = "x86_64-unknown-linux-musl";
    let status = Command::new("cargo")
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

    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/hyperion-agent-runtime has a workspace root two levels up")
        .to_path_buf();
    workspace_root
        .join("target")
        .join(target)
        .join("debug")
        .join("uppercase_tool")
}

#[test]
fn invoke_dispatches_an_unrecognized_capability_to_a_real_installed_plugin() {
    let scratch = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&scratch.path().join("device.key")).unwrap();

    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();

    let mut plugin_manifest = PluginManifest {
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
                program: uppercase_tool_bin(),
                args: vec![],
            }),
            privacy_tier: PrivacyTier::Local,
        })],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::Execute,
            scope: "text.uppercase".to_string(),
            justification: "run the real sandboxed tool".to_string(),
        }],
        min_trust_depth: TrustDepth::D1,
    };
    plugin_manifest.signature = Some(sign(&plugin_manifest, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            plugin_manifest,
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));
    let runtime =
        AgentRuntime::new_with_netstack_and_plugins(ai_runtime, None, Some(Arc::new(registry)));

    let agent_manifest = AgentManifest {
        specialization: "tool-user".to_string(),
        baseline_capabilities: vec!["text.uppercase".to_string()],
        requestable_capabilities: vec![],
        trust_tier: TrustTier::System,
    };
    let instance_id = runtime
        .spawn(&monitor, &root, agent_manifest, Some(1))
        .unwrap();

    let outcome = runtime
        .invoke(
            &monitor,
            &root,
            instance_id,
            "text.uppercase",
            json!({"text": "hello from a real plugin"}),
        )
        .unwrap();

    let InvokeOutcome::Result(result) = outcome else {
        panic!("expected a real Result outcome, got: {outcome:?}");
    };
    assert_eq!(
        result.get("text").and_then(|v| v.as_str()),
        Some("HELLO FROM A REAL PLUGIN"),
        "expected the real sandboxed plugin's real output (not stubs::dispatch's echo), got: \
         {result:?}"
    );
}

#[test]
fn invoke_falls_back_to_the_stub_echo_when_no_plugin_registry_is_wired() {
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));
    let runtime = AgentRuntime::new(ai_runtime);
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);

    let agent_manifest = AgentManifest {
        specialization: "tool-user".to_string(),
        baseline_capabilities: vec!["some.unknown.capability".to_string()],
        requestable_capabilities: vec![],
        trust_tier: TrustTier::System,
    };
    let instance_id = runtime
        .spawn(&monitor, &root, agent_manifest, Some(1))
        .unwrap();

    let outcome = runtime
        .invoke(
            &monitor,
            &root,
            instance_id,
            "some.unknown.capability",
            json!({"probe": "value"}),
        )
        .unwrap();

    let InvokeOutcome::Result(result) = outcome else {
        panic!("expected a real Result outcome, got: {outcome:?}");
    };
    assert_eq!(
        result.get("echo").and_then(|v| v.as_str()),
        Some("some.unknown.capability"),
        "with no plugin registry wired, an unrecognized capability must still fall back to the \
         real stub echo, got: {result:?}"
    );
}
