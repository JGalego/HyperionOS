//! docs/15 §7's own "blast-radius/sensitivity/reversibility classifiers"
//! deferral, narrowed: `verify_action` re-derives what real Knowledge-Graph
//! state can actually corroborate, rather than trusting a caller's own
//! claim — "a caller can claim low risk for anything" is exactly the gap
//! this closes for these three dimensions.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_security::{verify_action, PendingAction, SensitivityHint};

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    Arc<KnowledgeGraph>,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    (dir, monitor, token, graph)
}

fn claim(object_refs: Vec<hyperion_knowledge_graph::NodeId>, scope_size: u32) -> PendingAction {
    PendingAction {
        action_id: 1,
        object_refs,
        scope_size,
        reversible: true,
        sensitivity: SensitivityHint::Public,
        intent_confidence: 1.0,
        corroboration: 1.0,
        provenance: None,
    }
}

#[test]
fn scope_size_is_always_the_real_object_refs_length_never_the_claim() {
    let (_dir, monitor, token, graph) = setup();
    let real_objects: Vec<_> = (0..3)
        .map(|_| {
            graph
                .put_node(&monitor, &token, None, "Note", None, serde_json::json!({}))
                .unwrap()
        })
        .collect();

    // Claims a wildly understated scope_size (1) against a real 3-object list.
    let action = claim(real_objects, 1);
    let verified = verify_action(&monitor, &token, &graph, action);
    assert_eq!(
        verified.scope_size, 3,
        "the real object_refs length always wins"
    );

    // Claims a wildly overstated scope_size (1_000_000) against an empty real list.
    let action = claim(Vec::new(), 1_000_000);
    let verified = verify_action(&monitor, &token, &graph, action);
    assert_eq!(verified.scope_size, 0);
}

#[test]
fn a_reference_to_a_real_owned_object_keeps_the_callers_own_claims() {
    let (_dir, monitor, token, graph) = setup();
    let object = graph
        .put_node(&monitor, &token, None, "Note", None, serde_json::json!({}))
        .unwrap();
    let action = claim(vec![object], 1);
    let verified = verify_action(&monitor, &token, &graph, action);
    assert!(
        verified.reversible,
        "a real, verifiable object keeps the caller's true claim"
    );
    assert_eq!(verified.sensitivity, SensitivityHint::Public);
}

#[test]
fn a_reference_to_a_nonexistent_object_is_never_trusted_as_reversible_or_low_sensitivity() {
    let (_dir, monitor, token, graph) = setup();
    let phantom = hyperion_storage::ObjectId(999_999);
    let action = claim(vec![phantom], 1);
    let verified = verify_action(&monitor, &token, &graph, action);
    assert!(
        !verified.reversible,
        "claiming reversibility about an object that doesn't exist is an unverified claim"
    );
    assert_eq!(
        verified.sensitivity,
        SensitivityHint::Sensitive,
        "an unverifiable reference is escalated, not trusted at face value"
    );
}

#[test]
fn a_reference_to_a_foreign_boundarys_object_is_never_trusted_either() {
    let (_dir, monitor, token, graph) = setup();
    let other = {
        let mut m2 = CapabilityMonitor::new();
        let other_token = m2.mint_root(RightsMask::all(), TrustBoundaryId(2), None);
        graph
            .put_node(&m2, &other_token, None, "Note", None, serde_json::json!({}))
            .unwrap()
    };
    let action = claim(vec![other], 1);
    let verified = verify_action(&monitor, &token, &graph, action);
    assert!(
        !verified.reversible,
        "a foreign-boundary object is indistinguishable from nonexistent -- never trusted"
    );
    assert_eq!(verified.sensitivity, SensitivityHint::Sensitive);
}

#[test]
fn an_escalated_sensitivity_claim_is_never_downgraded() {
    let (_dir, monitor, token, graph) = setup();
    let object = graph
        .put_node(&monitor, &token, None, "Note", None, serde_json::json!({}))
        .unwrap();
    let mut action = claim(vec![object], 1);
    action.sensitivity = SensitivityHint::Restricted;
    let verified = verify_action(&monitor, &token, &graph, action);
    assert_eq!(
        verified.sensitivity,
        SensitivityHint::Restricted,
        "verification only ever escalates, never downgrades a caller's own higher claim"
    );
}

#[test]
fn no_object_refs_at_all_is_vacuously_verified_not_escalated() {
    let (_dir, monitor, token, graph) = setup();
    let action = claim(Vec::new(), 0);
    let verified = verify_action(&monitor, &token, &graph, action);
    assert!(verified.reversible);
    assert_eq!(verified.sensitivity, SensitivityHint::Public);
}
