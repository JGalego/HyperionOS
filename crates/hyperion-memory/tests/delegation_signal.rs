//! docs/998-roadmap.md's Backlog "Protect the Human" item:
//! `MemoryEngine::count_procedural_delegations` really counts how many times a specific kind of
//! task (a caller-supplied `entity_key`) has been delegated, filtered to the Procedural tier and
//! a caller-supplied time window — never a decision, just a real, honest count.

use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_memory::{MemoryEngine, MemoryFilter, MemoryTier};
use serde_json::json;

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    MemoryEngine,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let engine = MemoryEngine::new(graph);
    (dir, monitor, token, engine)
}

#[test]
fn counts_only_procedural_records_with_the_matching_entity_key() {
    let (_dir, monitor, token, engine) = setup();

    for _ in 0..3 {
        engine
            .remember(
                &monitor,
                &token,
                MemoryTier::Procedural,
                json!({"entity_key": "export.png"}),
                None,
                0.5,
                false,
                Vec::new(),
            )
            .unwrap();
    }
    // A different entity_key -- must not be counted.
    engine
        .remember(
            &monitor,
            &token,
            MemoryTier::Procedural,
            json!({"entity_key": "export.jpg"}),
            None,
            0.5,
            false,
            Vec::new(),
        )
        .unwrap();
    // Same entity_key, wrong tier -- must not be counted either.
    engine
        .remember(
            &monitor,
            &token,
            MemoryTier::Semantic,
            json!({"entity_key": "export.png"}),
            None,
            0.5,
            false,
            Vec::new(),
        )
        .unwrap();

    let delegation = engine
        .count_procedural_delegations(&monitor, &token, "export.png", 0)
        .unwrap();
    assert_eq!(delegation.count, 3);
    assert_eq!(delegation.entity_key, "export.png");
}

#[test]
fn since_ts_excludes_delegations_from_before_the_window() {
    let (_dir, monitor, token, engine) = setup();

    engine
        .remember(
            &monitor,
            &token,
            MemoryTier::Procedural,
            json!({"entity_key": "export.png"}),
            None,
            0.5,
            false,
            Vec::new(),
        )
        .unwrap();

    sleep(Duration::from_secs(2));
    let window_start = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    sleep(Duration::from_secs(1));

    engine
        .remember(
            &monitor,
            &token,
            MemoryTier::Procedural,
            json!({"entity_key": "export.png"}),
            None,
            0.5,
            false,
            Vec::new(),
        )
        .unwrap();

    // Sanity: both really exist before windowing.
    let unwindowed = engine
        .count_procedural_delegations(&monitor, &token, "export.png", 0)
        .unwrap();
    assert_eq!(unwindowed.count, 2);

    let windowed = engine
        .count_procedural_delegations(&monitor, &token, "export.png", window_start)
        .unwrap();
    assert_eq!(
        windowed.count, 1,
        "only the delegation created after window_start should count"
    );
}

#[test]
fn erased_procedural_records_are_not_counted() {
    let (_dir, monitor, token, engine) = setup();

    let id = engine
        .remember(
            &monitor,
            &token,
            MemoryTier::Procedural,
            json!({"entity_key": "export.png"}),
            None,
            0.5,
            false,
            Vec::new(),
        )
        .unwrap();
    engine.erase(&monitor, &token, id, false).unwrap();

    let delegation = engine
        .count_procedural_delegations(&monitor, &token, "export.png", 0)
        .unwrap();
    assert_eq!(delegation.count, 0);

    // Also confirm the plain query-with-erased-included still sees it, proving this is really
    // filtered by MemoryFilter's default (not merely coincidentally zero).
    let all = engine
        .query(
            &monitor,
            &token,
            &MemoryFilter {
                tier: Some(MemoryTier::Procedural),
                include_erased: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(all.len(), 1);
}
