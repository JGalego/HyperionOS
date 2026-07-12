//! PRODUCTION_BOOT_PROMPT.md M13: docs/41 Phase 10's literal exit criterion -- "a staged update
//! applied to a real running booted system and rolled back without data loss" -- finally tested
//! against a real system (this process, inside the real booted QEMU guest) rather than
//! `hyperion-update`'s own existing in-process test harness (`tests/staged_rollout.rs`), which
//! proves the same algorithm but never against a real, separate process with real capability
//! enforcement and a real file-backed Knowledge Graph.
//!
//! Opt-in via a `hyperion.run_update_test=1` kernel cmdline parameter (read from the real
//! `/proc/cmdline`, the same way a real init process actually receives boot parameters) so this
//! is completely inert on every other boot -- M7/M11/M12's own boot tests never pass this flag.
//!
//! Self-contained within one boot, unlike [`crate::storage_probe`]'s own crash-consistency test:
//! there is no crash/interruption semantics to prove here, just a real, signed update applied and
//! then really rolled back. Writes a real node to a dedicated real
//! [`hyperion_knowledge_graph::KnowledgeGraph`] on the persistent data partition, applies a real
//! [`hyperion_update::UpdateOrchestrator::apply_update`] against it (real Ed25519 signature
//! verification via [`hyperion_crypto`], a real health-gated staged rollout, a real
//! [`hyperion_recovery::RecoveryService`] pre-update snapshot), writes new data to that node
//! representing what the update's own payload changed (the orchestrator itself has no migration
//! DSL -- see that crate's own doc comment -- applying the actual change is the caller's job, same
//! as any real update would need to do), then calls a real `update_rollback`, which really
//! restores the pre-update snapshot via `RecoveryService::restore_to`. Reports a single
//! greppable pass/fail line to the serial console for
//! `boot/scripts/update-rollback-test.sh` to scrape.

use std::path::Path;
use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_recovery::RecoveryService;
use hyperion_update::{
    sign, CohortHealth, HealthThresholds, RolloutPolicy, UpdateManifest, UpdateOrchestrator,
    UpdateSubject,
};

const UPDATE_TEST_GRAPH_PATH: &str = "update_test.kg.jsonl";
const PRE_UPDATE_VALUE: &str = "pre-update-original";
const POST_UPDATE_VALUE: &str = "POST-UPDATE-MODIFIED";

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn should_run() -> bool {
    std::fs::read_to_string("/proc/cmdline")
        .map(|cmdline| {
            cmdline
                .split_whitespace()
                .any(|arg| arg == "hyperion.run_update_test=1")
        })
        .unwrap_or(false)
}

/// Runs the real update/rollback proof if (and only if) this boot's own `/proc/cmdline` opted in.
pub fn run_update_rollback_probe(data_dir: &Path) {
    if !should_run() {
        return;
    }

    let graph = match KnowledgeGraph::open(data_dir.join(UPDATE_TEST_GRAPH_PATH)) {
        Ok(g) => Arc::new(g),
        Err(e) => {
            println!(
                "[hyperion-init] UPDATE_TEST: FAIL -- couldn't open the real knowledge graph: {e}"
            );
            return;
        }
    };
    let keystore = match Keystore::open_or_create(&data_dir.join("device.key")) {
        Ok(k) => k,
        Err(e) => {
            println!("[hyperion-init] UPDATE_TEST: FAIL -- couldn't open the real keystore: {e}");
            return;
        }
    };

    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(0), None);

    let node_id = match graph.put_node(
        &monitor,
        &token,
        None,
        "update_test_object",
        None,
        serde_json::json!({ "value": PRE_UPDATE_VALUE }),
    ) {
        Ok(id) => id,
        Err(e) => {
            println!(
                "[hyperion-init] UPDATE_TEST: FAIL -- couldn't write the real initial node: {e}"
            );
            return;
        }
    };

    let subject = UpdateSubject::SystemImage;
    // A fresh UpdateOrchestrator's `active_version` starts at 0 (its own `unwrap_or(0)` default,
    // never set by a constructor) -- `from_version` here must match that starting point, or
    // `compatibility_check`'s real schema-compatibility check correctly (not a bug) rejects the
    // manifest as stale before ever reaching the rollout/recovery-point logic this probe means to
    // exercise.
    let mut manifest = UpdateManifest {
        subject: subject.clone(),
        from_version: 0,
        to_version: 1,
        signature: None,
        touched_objects: vec![node_id],
        rollout_policy: RolloutPolicy::default_schedule(HealthThresholds {
            max_crash_rate: 0.05,
            max_latency_p99_ms: 500,
        }),
    };
    manifest.signature = Some(sign(&manifest, &keystore));

    let recovery = Arc::new(RecoveryService::new(graph.clone()));
    let orchestrator = UpdateOrchestrator::new(recovery);

    let applied_version = match orchestrator.apply_update(
        &monitor,
        &token,
        &manifest,
        true,
        now(),
        |_percent| CohortHealth {
            crash_rate: 0.0,
            latency_p99_ms: 10,
        },
        &keystore.verifying_key(),
    ) {
        Ok(v) => v,
        Err(e) => {
            println!("[hyperion-init] UPDATE_TEST: FAIL -- real apply_update failed: {e}");
            return;
        }
    };

    // The orchestrator itself has no migration DSL (see hyperion-update's own doc comment) --
    // applying the update's own real payload change to its touched data is the caller's job, same
    // as any real update would need to do.
    if let Err(e) = graph.put_node(
        &monitor,
        &token,
        Some(node_id),
        "update_test_object",
        None,
        serde_json::json!({ "value": POST_UPDATE_VALUE }),
    ) {
        println!(
            "[hyperion-init] UPDATE_TEST: FAIL -- couldn't write the real post-update data: {e}"
        );
        return;
    }

    let post_update_value = match graph.get(&monitor, &token, node_id) {
        Ok(record) => record.metadata["value"].as_str().unwrap_or("").to_string(),
        Err(e) => {
            println!(
                "[hyperion-init] UPDATE_TEST: FAIL -- couldn't read back post-update data: {e}"
            );
            return;
        }
    };
    println!(
        "[hyperion-init] UPDATE_TEST: applied real update to v{applied_version}, real data now = \
         {post_update_value:?}"
    );

    let receipt = match orchestrator.update_rollback(&monitor, &token, &manifest) {
        Ok(r) => r,
        Err(e) => {
            println!("[hyperion-init] UPDATE_TEST: FAIL -- real update_rollback failed: {e}");
            return;
        }
    };

    let restored_value = match graph.get(&monitor, &token, node_id) {
        Ok(record) => record.metadata["value"].as_str().unwrap_or("").to_string(),
        Err(e) => {
            println!("[hyperion-init] UPDATE_TEST: FAIL -- couldn't read back restored data: {e}");
            return;
        }
    };
    let active_version = orchestrator.active_version(&subject);

    if restored_value == PRE_UPDATE_VALUE
        && receipt.rolled_back_to == manifest.from_version
        && active_version == manifest.from_version
    {
        println!(
            "[hyperion-init] UPDATE_TEST: PASS -- real rollback restored real data to \
             {restored_value:?} (no data loss), real active version back to v{active_version}"
        );
    } else {
        println!(
            "[hyperion-init] UPDATE_TEST: FAIL -- data loss or version mismatch after real \
             rollback (data={restored_value:?}, version={active_version})"
        );
    }
}
