//! Mirrors every other crate in this workspace: every call is capability-
//! gated, re-checked live against the monitor, never cached.

use std::sync::Arc;

use hyperion_agent_runtime::{AgentError, AgentManifest, AgentRuntime, TrustTier};
use hyperion_ai_runtime::{LocalAiRuntime, MockBackend};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use serde_json::json;

fn manifest() -> AgentManifest {
    AgentManifest {
        specialization: "research".to_string(),
        baseline_capabilities: vec!["web.search".to_string()],
        requestable_capabilities: vec![],
        trust_tier: TrustTier::System,
    }
}

fn new_runtime() -> AgentRuntime {
    AgentRuntime::new(Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000)))
}

#[test]
fn spawn_requires_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();

    let runtime = new_runtime();
    let result = runtime.spawn(&monitor, &read_only, manifest(), None);
    assert!(matches!(result, Err(AgentError::Unauthorized)));
}

#[test]
fn invoke_requires_exec_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let write_only = monitor
        .cap_derive(&root, RightsMask::WRITE, None, TrustBoundaryId(2))
        .unwrap();

    let runtime = new_runtime();
    let id = runtime.spawn(&monitor, &root, manifest(), None).unwrap();
    let result = runtime.invoke(&monitor, &write_only, id, "web.search", json!({}));
    assert!(matches!(result, Err(AgentError::Unauthorized)));
}

#[test]
fn revoking_a_token_blocks_further_access_re_checked_live() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let delegate = monitor
        .cap_derive(
            &root,
            RightsMask::READ | RightsMask::WRITE | RightsMask::EXEC,
            None,
            TrustBoundaryId(2),
        )
        .unwrap();

    let runtime = new_runtime();
    let id = runtime
        .spawn(&monitor, &delegate, manifest(), None)
        .unwrap();
    assert!(runtime
        .invoke(&monitor, &delegate, id, "web.search", json!({}))
        .is_ok());

    monitor.cap_revoke(&delegate);

    assert!(matches!(
        runtime.invoke(&monitor, &delegate, id, "web.search", json!({})),
        Err(AgentError::Unauthorized)
    ));
}
