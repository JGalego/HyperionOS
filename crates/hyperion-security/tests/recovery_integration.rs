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
    RecoveryService,
) {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let recovery = RecoveryService::new(graph);
    (monitor, root, recovery)
}

#[test]
fn a_backup_first_action_gets_a_recovery_point_before_it_proceeds() {
    let (monitor, root, recovery) = setup();
    let action = PendingAction {
        action_id: 1,
        object_refs: vec![hyperion_storage::ObjectId(1), hyperion_storage::ObjectId(2)],
        scope_size: 100,
        reversible: false,
        sensitivity: SensitivityHint::Sensitive,
        intent_confidence: 0.9,
        corroboration: 0.2,
        provenance: None,
    };

    let assessment = assess_and_prepare(&monitor, &root, &recovery, &action, 1_000).unwrap();
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
    let (monitor, root, recovery) = setup();
    let action = PendingAction {
        action_id: 1,
        object_refs: vec![hyperion_storage::ObjectId(1)],
        scope_size: 1,
        reversible: true,
        sensitivity: SensitivityHint::Public,
        intent_confidence: 1.0,
        corroboration: 1.0,
        provenance: None,
    };

    let assessment = assess_and_prepare(&monitor, &root, &recovery, &action, 1_000).unwrap();
    assert_eq!(
        assessment.intervention_level,
        InterventionLevel::SilentProceed
    );
    assert!(assessment.recovery_point_ref.is_none());
    assert!(recovery.action_records().is_empty());
}
