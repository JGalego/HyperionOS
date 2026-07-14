//! docs/06 §5.4's `ExpertiseEstimate`: a real, working-set-activity-derived
//! signal (see the crate doc's Adaptive Complexity narrowing) rather than
//! the always-fixed `Novice`/zero-confidence stub this method used to
//! return unconditionally regardless of real session activity.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::{Budget, ContextEngine, ExpertiseLevel, Scope};
use hyperion_knowledge_graph::KnowledgeGraph;
use serde_json::json;

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    (dir, monitor, token)
}

#[test]
fn a_session_with_no_activity_yet_reports_the_honest_zero_confidence_default() {
    let (dir, _monitor, _token) = setup();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let engine = ContextEngine::new(graph);

    let estimate = engine.current_expertise("never-seen-session", "general");
    assert_eq!(estimate.level, ExpertiseLevel::Novice);
    assert_eq!(estimate.confidence, 0.0);
}

#[test]
fn repeated_engagement_in_one_session_produces_a_real_escalating_signal() {
    let (dir, monitor, token) = setup();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let engine = ContextEngine::new(graph.clone());

    let anchors: Vec<_> = (0..5)
        .map(|i| {
            graph
                .put_node(
                    &monitor,
                    &token,
                    None,
                    "repository",
                    None,
                    json!({"name": format!("repo-{i}")}),
                )
                .unwrap()
        })
        .collect();

    let scope = Scope {
        intent_id: "i".into(),
        session_id: "s-active".into(),
        mentions: Vec::new(),
        anchors: anchors.clone(),
    };

    // First assemble: 5 distinct entries touched once each -- Intermediate
    // (distinct_entries=5, total_hits=5, sum=10, within the 5..=14 band).
    engine
        .assemble(&monitor, &token, &scope, Budget::default())
        .unwrap();
    let after_one = engine.current_expertise("s-active", "general");
    assert_eq!(after_one.level, ExpertiseLevel::Intermediate);
    assert!(after_one.confidence > 0.0);
    assert!(after_one.evidence[0].contains("5 distinct working-set entries"));

    // Second assemble against the same anchors: the same 5 entries are now
    // hit twice each -- sum=20, within the 15..=29 Advanced band.
    engine
        .assemble(&monitor, &token, &scope, Budget::default())
        .unwrap();
    let after_two = engine.current_expertise("s-active", "general");
    assert_eq!(after_two.level, ExpertiseLevel::Advanced);
    assert!(after_two.confidence > after_one.confidence);

    // A different, never-touched session reports the same honest default --
    // this is real per-session state, not a global counter leaking across
    // sessions.
    let untouched = engine.current_expertise("s-other", "general");
    assert_eq!(untouched.level, ExpertiseLevel::Novice);
    assert_eq!(untouched.confidence, 0.0);
}
