//! One device, more than one person (docs/998-roadmap.md §0, Decision 2): a memory belongs to
//! whoever wrote it, and nobody else can read, change, or erase it.
//!
//! This mirrors the rule `hyperion_explainability::ExplanationStore` already applied to its own
//! records. That crate was built to tell two callers apart; this one was not, which is why every
//! read here used to hand back everything regardless of who asked.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{KnowledgeGraph, NodeId};
use hyperion_memory::{MemoryEngine, MemoryFilter, MemoryTier};

/// Two people on one device, with the boundaries `hyperion-identity` really allocates.
const ALICE: TrustBoundaryId = TrustBoundaryId(1_000);
const BOB: TrustBoundaryId = TrustBoundaryId(1_001);

struct Device {
    monitor: CapabilityMonitor,
    alice: CapabilityToken,
    bob: CapabilityToken,
    memory: MemoryEngine,
    _dir: tempfile::TempDir,
}

fn device() -> Device {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let alice = monitor.mint_root(RightsMask::all(), ALICE, None);
    let bob = monitor.mint_root(RightsMask::all(), BOB, None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    Device {
        monitor,
        alice,
        bob,
        memory: MemoryEngine::new(graph),
        _dir: dir,
    }
}

fn remember(device: &Device, token: &CapabilityToken, what: &str) -> NodeId {
    device
        .memory
        .remember(
            &device.monitor,
            token,
            MemoryTier::Semantic,
            serde_json::json!({ "entity_key": "note", "text": what }),
            None,
            0.9,
            true,
            Vec::new(),
        )
        .expect("remembering really works")
}

#[test]
fn one_persons_memories_are_not_in_anothers_results() {
    let device = device();
    remember(&device, &device.alice, "heliotrope");

    let alice_sees = device
        .memory
        .query(&device.monitor, &device.alice, &MemoryFilter::default())
        .unwrap();
    assert_eq!(alice_sees.len(), 1, "Alice must see her own note");

    let bob_sees = device
        .memory
        .query(&device.monitor, &device.bob, &MemoryFilter::default())
        .unwrap();
    assert!(
        bob_sees.is_empty(),
        "Bob must not see Alice's memories, got: {bob_sees:?}"
    );
}

#[test]
fn an_export_carries_only_the_asking_persons_memories() {
    // `export` is docs/08's portability promise and runs through `query`, so it inherits the
    // filter -- but it is exactly the operation where leaking someone else's data would be worst.
    let device = device();
    remember(&device, &device.alice, "heliotrope");
    remember(&device, &device.bob, "vermilion");

    let bobs_export = device
        .memory
        .export(&device.monitor, &device.bob, &MemoryFilter::default())
        .unwrap()
        .to_string();
    assert!(bobs_export.contains("vermilion"), "got: {bobs_export}");
    assert!(!bobs_export.contains("heliotrope"), "got: {bobs_export}");
}

#[test]
fn a_record_cannot_be_reached_by_guessing_its_id() {
    // Without this, the read filters above would be decorative: anything reachable by id would be
    // readable, editable and erasable by anyone who guessed one.
    let device = device();
    let alices = remember(&device, &device.alice, "heliotrope");

    assert!(
        device
            .memory
            .edit(
                &device.monitor,
                &device.bob,
                alices,
                serde_json::json!({ "text": "tampered" })
            )
            .is_err(),
        "Bob must not be able to edit Alice's memory"
    );
    assert!(
        device
            .memory
            .erase(&device.monitor, &device.bob, alices, false)
            .is_err(),
        "Bob must not be able to erase Alice's memory"
    );
    assert!(
        device
            .memory
            .pin(&device.monitor, &device.bob, alices)
            .is_err(),
        "Bob must not be able to pin Alice's memory"
    );
    assert!(
        device
            .memory
            .explain(&device.monitor, &device.bob, alices)
            .is_err(),
        "where a memory came from is as much its owner's business as the memory"
    );

    // Alice's own record is untouched by every one of those attempts.
    let alice_sees = device
        .memory
        .query(&device.monitor, &device.alice, &MemoryFilter::default())
        .unwrap();
    assert_eq!(alice_sees.len(), 1);
    assert_eq!(alice_sees[0].content["text"], "heliotrope");
    assert!(!alice_sees[0].erased);
}

#[test]
fn the_owner_can_still_do_all_of_that_to_their_own() {
    // The filter has to stop the wrong person without getting in the right one's way.
    let device = device();
    let mine = remember(&device, &device.alice, "heliotrope");

    assert!(device
        .memory
        .edit(
            &device.monitor,
            &device.alice,
            mine,
            serde_json::json!({ "text": "still mine" })
        )
        .is_ok());
    assert!(device
        .memory
        .pin(&device.monitor, &device.alice, mine)
        .is_ok());
    assert!(device
        .memory
        .unpin(&device.monitor, &device.alice, mine)
        .is_ok());
    assert!(device
        .memory
        .explain(&device.monitor, &device.alice, mine)
        .is_ok());
    assert!(device
        .memory
        .erase(&device.monitor, &device.alice, mine, false)
        .is_ok());
}

#[test]
fn erasing_with_cascade_never_reaches_across_to_another_person() {
    // `erase(cascade)` walks `query` for dependents. With no filter that walk crossed people, so
    // erasing your own record could soft-delete someone else's.
    let device = device();
    let alices = remember(&device, &device.alice, "heliotrope");

    let bobs_dependent = device
        .memory
        .remember(
            &device.monitor,
            &device.bob,
            MemoryTier::Semantic,
            serde_json::json!({ "entity_key": "note", "text": "vermilion" }),
            None,
            0.9,
            true,
            // Bob's record names Alice's as its provenance, which is what a cascade follows.
            vec![alices],
        )
        .unwrap();

    device
        .memory
        .erase(&device.monitor, &device.alice, alices, true)
        .unwrap();

    let bob_sees = device
        .memory
        .query(&device.monitor, &device.bob, &MemoryFilter::default())
        .unwrap();
    let survived = bob_sees.iter().find(|r| r.id == bobs_dependent);
    assert!(
        survived.is_some_and(|r| !r.erased),
        "Alice's cascade must not erase Bob's record, got: {bob_sees:?}"
    );
}

#[test]
fn a_record_written_before_this_existed_belongs_to_nobody_rather_than_everybody() {
    // Records written before `origin_boundary` existed deserialize to 0. No real caller holds
    // boundary 0 -- principals start at 1000 and hand-minted roots at 1 -- so such a record is
    // readable by nobody. Hiding an old development record is recoverable; showing one person
    // another's memories is not.
    let mut device = device();
    let unattributed = device
        .monitor
        .mint_root(RightsMask::all(), TrustBoundaryId(0), None);
    remember(&device, &unattributed, "from before");

    for (who, token) in [("alice", &device.alice), ("bob", &device.bob)] {
        let seen = device
            .memory
            .query(&device.monitor, token, &MemoryFilter::default())
            .unwrap();
        assert!(
            seen.is_empty(),
            "{who} must not inherit an unattributed record"
        );
    }
}
