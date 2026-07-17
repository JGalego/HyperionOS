//! docs/18-explainability-and-trust.md §9: "Explanation store unavailable.
//! `explain.query` degrades to `best_effort_reconstruction`, replaying
//! [31 — Event System] logs to approximate the record and flagging the
//! result as reconstructed, never presenting a best-effort guess as an
//! authoritative record." Simulates exactly that: a second, independent
//! `ExplanationStore` (standing in for "the real store is gone/restarted")
//! that never saw a record live, reconstructing it purely from the durable
//! Event System log a *different* store instance published to.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_events::EventBus;
use hyperion_explainability::{ControlState, ExplanationStore, ReasoningStep};

fn admin_and_actor(
    monitor: &mut CapabilityMonitor,
    boundary: TrustBoundaryId,
) -> (
    hyperion_capability::CapabilityToken,
    hyperion_capability::CapabilityToken,
) {
    let admin = monitor.mint_root(RightsMask::READ | RightsMask::GRANT, boundary, None);
    let actor = monitor.mint_root(RightsMask::READ | RightsMask::WRITE, boundary, None);
    (admin, actor)
}

#[test]
fn a_fresh_store_reconstructs_a_record_it_never_saw_live() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let boundary = TrustBoundaryId(1);
    let (admin, actor) = admin_and_actor(&mut monitor, boundary);

    let id;
    let action_id = 42;
    {
        let bus = Arc::new(EventBus::new(Some(dir.path().to_path_buf())));
        let store = ExplanationStore::new()
            .with_events(&monitor, &admin, bus)
            .unwrap();

        id = store
            .begin(
                &monitor,
                &actor,
                action_id,
                7,
                9,
                "document.draft",
                vec![],
                1_000,
            )
            .unwrap();
        store
            .append_step(
                &monitor,
                &actor,
                id,
                ReasoningStep {
                    step_index: 0,
                    description: "drafted the document".to_string(),
                    capability_ref: Some("document.draft".to_string()),
                    inputs_ref: vec![],
                    output_ref: None,
                },
                vec![],
            )
            .unwrap();
        store
            .transition(&monitor, &actor, id, ControlState::Completed)
            .unwrap();
        // `store` (standing in for the real in-memory ExplanationStore) is
        // dropped here -- everything it knew about this record, other than
        // what it published, is gone.
    }

    // A second, completely independent bus + store, backed by the same durable
    // directory -- the "the real store crashed and this is what's left" scenario.
    let bus2 = Arc::new(EventBus::new(Some(dir.path().to_path_buf())));
    let store2 = ExplanationStore::new()
        .with_events(&monitor, &admin, bus2)
        .unwrap();

    let lookup = store2
        .get_or_reconstruct(&monitor, &actor, id)
        .unwrap()
        .expect("a real, published record should be reconstructable");
    assert!(lookup.is_reconstructed());
    let record = lookup.record();
    assert_eq!(record.action_id, action_id);
    assert_eq!(record.agent_id, 9);
    assert_eq!(record.capability_ref, "document.draft");
    assert_eq!(record.reasoning_chain.len(), 1);
    assert_eq!(
        record.reasoning_chain[0].description,
        "drafted the document"
    );
    assert_eq!(record.control_state, ControlState::Completed);

    // Resolving by the real action_id (the caller's actual entry point,
    // per docs/18 §5/§6) works too, without knowing the internal id at all.
    let by_action = store2
        .get_or_reconstruct_by_action(&monitor, &actor, action_id)
        .unwrap()
        .unwrap();
    assert!(by_action.is_reconstructed());
    assert_eq!(by_action.record().id, id);
}

#[test]
fn reconstruction_never_crosses_a_trust_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let (admin, actor) = admin_and_actor(&mut monitor, TrustBoundaryId(1));
    let foreign = monitor.mint_root(RightsMask::READ, TrustBoundaryId(2), None);

    let id;
    {
        let bus = Arc::new(EventBus::new(Some(dir.path().to_path_buf())));
        let store = ExplanationStore::new()
            .with_events(&monitor, &admin, bus)
            .unwrap();
        id = store
            .begin(&monitor, &actor, 1, 1, 1, "document.draft", vec![], 1_000)
            .unwrap();
    }

    let bus2 = Arc::new(EventBus::new(Some(dir.path().to_path_buf())));
    let store2 = ExplanationStore::new()
        .with_events(&monitor, &admin, bus2)
        .unwrap();

    // The admin subscription can see the raw event (it's kind-wide), but
    // `get_or_reconstruct` must still refuse a caller from a different
    // Trust Boundary than the one that actually opened this record.
    let denied = store2.get_or_reconstruct(&monitor, &foreign, id).unwrap();
    assert!(denied.is_none());

    let allowed = store2.get_or_reconstruct(&monitor, &actor, id).unwrap();
    assert!(allowed.is_some());
}

#[test]
fn a_store_with_no_wired_bus_and_no_record_returns_none() {
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(
        RightsMask::READ | RightsMask::WRITE,
        TrustBoundaryId(1),
        None,
    );
    let store = ExplanationStore::new();
    assert!(store
        .get_or_reconstruct(&monitor, &token, 999)
        .unwrap()
        .is_none());
}
