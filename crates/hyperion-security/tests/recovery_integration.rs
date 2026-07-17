//! docs/15's relationship to docs/33: `assess_and_prepare` calls into
//! `hyperion-recovery` synchronously, before any `RequireBackupFirst`
//! action proceeds — the recovery point is a precondition, not an
//! afterthought.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_recovery::RecoveryService;
use hyperion_security::{assess_and_prepare, InterventionLevel, PendingAction, SensitivityHint};

fn setup() -> (
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    Arc<KnowledgeGraph>,
    RecoveryService,
) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let recovery = RecoveryService::new(graph.clone());
    (monitor, root, graph, recovery)
}

#[test]
fn a_backup_first_action_gets_a_recovery_point_before_it_proceeds() {
    let (monitor, root, graph, recovery) = setup();
    // Real, existing objects -- `verify_action` re-derives `scope_size` from this real list and
    // would otherwise reject a claimed count it can't corroborate.
    let object_refs: Vec<_> = (0..10)
        .map(|_| {
            graph
                .put_node(&monitor, &root, None, "Note", None, serde_json::json!({}))
                .unwrap()
        })
        .collect();
    let action = PendingAction {
        action_id: 1,
        object_refs,
        scope_size: 100,
        reversible: false,
        sensitivity: SensitivityHint::Sensitive,
        intent_confidence: 0.9,
        corroboration: 0.2,
        provenance: None,
    };

    let assessment =
        assess_and_prepare(&monitor, &root, &graph, &recovery, &action, 1_000).unwrap();
    assert_eq!(
        assessment.intervention_level,
        InterventionLevel::RequireBackupFirst
    );
    let rp = assessment
        .recovery_point_ref
        .expect("a backup-first action must carry a recovery point reference");
    assert!(recovery.recovery_point(rp).is_some());
}

#[test]
fn a_silent_proceed_action_never_creates_a_recovery_point() {
    let (monitor, root, graph, recovery) = setup();
    let object_id = graph
        .put_node(&monitor, &root, None, "Note", None, serde_json::json!({}))
        .unwrap();
    let action = PendingAction {
        action_id: 1,
        object_refs: vec![object_id],
        scope_size: 1,
        reversible: true,
        sensitivity: SensitivityHint::Public,
        intent_confidence: 1.0,
        corroboration: 1.0,
        provenance: None,
    };

    let assessment =
        assess_and_prepare(&monitor, &root, &graph, &recovery, &action, 1_000).unwrap();
    assert_eq!(
        assessment.intervention_level,
        InterventionLevel::SilentProceed
    );
    assert!(assessment.recovery_point_ref.is_none());
    assert!(recovery.action_records().is_empty());
}

#[test]
fn an_action_referencing_an_unverifiable_object_is_never_treated_as_safely_reversible() {
    let (monitor, root, graph, recovery) = setup();
    // A claimed-safe action over an object that was never actually created --
    // `verify_action` must not take the caller's word for it.
    let action = PendingAction {
        action_id: 1,
        object_refs: vec![hyperion_storage::ObjectId(999)],
        scope_size: 1,
        reversible: true,
        sensitivity: SensitivityHint::Public,
        intent_confidence: 1.0,
        corroboration: 1.0,
        provenance: None,
    };

    let assessment =
        assess_and_prepare(&monitor, &root, &graph, &recovery, &action, 1_000).unwrap();
    assert_ne!(
        assessment.intervention_level,
        InterventionLevel::SilentProceed,
        "a reference to an object this caller cannot verify must never score as safely reversible"
    );
}
