//! docs/06 §5.4's `ExpertiseEstimate`: a real, working-set-activity-derived
//! signal (see the crate doc's Adaptive Complexity narrowing) rather than
//! the always-fixed `Novice`/zero-confidence stub this method used to
//! return unconditionally regardless of real session activity.

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::{
    Budget, CapabilityTierReach, ContextEngine, ErrorRecoveryPattern, ExpertiseLevel,
    ExpertiseSignal, Scope,
};
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

/// docs/06 §5.4's own fuller Adaptive Complexity read: pushing a real vocabulary-complexity
/// sample (the signal `hyperion-intent::IntentEngine::handle_utterance` pushes for every real
/// utterance) raises the estimate and names it in `evidence`, even with no working-set activity
/// at all.
#[test]
fn a_pushed_vocabulary_complexity_signal_is_named_in_the_evidence_and_raises_the_estimate() {
    let (dir, _monitor, _token) = setup();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let engine = ContextEngine::new(graph);

    let before = engine.current_expertise("s-vocab", "general");
    assert_eq!(before.level, ExpertiseLevel::Novice);
    assert_eq!(before.confidence, 0.0);

    engine.record_expertise_signal("s-vocab", ExpertiseSignal::VocabularyComplexity(0.9));
    let after = engine.current_expertise("s-vocab", "general");
    assert!(
        after
            .evidence
            .iter()
            .any(|e| e.contains("vocabulary complexity")),
        "got: {:?}",
        after.evidence
    );
    assert!(after.confidence > before.confidence);
}

/// docs/06's own "Capability tier the user has been reaching for": reaching directly for a raw
/// Capability (never a guided workflow) is named as a real signal of higher expertise.
#[test]
fn a_pushed_raw_api_capability_tier_reach_is_named_in_the_evidence() {
    let (dir, _monitor, _token) = setup();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let engine = ContextEngine::new(graph);

    engine.record_expertise_signal(
        "s-tier",
        ExpertiseSignal::CapabilityTierReach(CapabilityTierReach::RawApi),
    );
    let estimate = engine.current_expertise("s-tier", "general");
    assert!(
        estimate
            .evidence
            .iter()
            .any(|e| e.contains("raw Capability directly")),
        "got: {:?}",
        estimate.evidence
    );
}

/// docs/06's own "does the user self-correct with technical vocabulary, or ask Hyperion to
/// explain?": a real `/redo`-shaped self-correction and a real `/teach`-shaped request for an
/// explanation must pull the estimate in opposite directions, not just be ignored either way.
#[test]
fn self_correction_and_asking_for_an_explanation_pull_the_estimate_in_opposite_directions() {
    let (dir, _monitor, _token) = setup();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());

    let self_correcting_engine = ContextEngine::new(graph.clone());
    for _ in 0..10 {
        self_correcting_engine.record_expertise_signal(
            "s-self-corrects",
            ExpertiseSignal::ErrorRecovery(ErrorRecoveryPattern::SelfCorrected),
        );
    }
    let self_correcting = self_correcting_engine.current_expertise("s-self-corrects", "general");

    let explanation_seeking_engine = ContextEngine::new(graph);
    for _ in 0..10 {
        explanation_seeking_engine.record_expertise_signal(
            "s-asks-for-help",
            ExpertiseSignal::ErrorRecovery(ErrorRecoveryPattern::AskedForExplanation),
        );
    }
    let explanation_seeking =
        explanation_seeking_engine.current_expertise("s-asks-for-help", "general");

    assert!(
        self_correcting.level > explanation_seeking.level,
        "self_correcting={:?} explanation_seeking={:?}",
        self_correcting.level,
        explanation_seeking.level
    );
}

/// All four real signals (working-set activity plus the three pushed samples) blend into one
/// estimate, all named in `evidence` -- not just whichever one happened to be pushed most
/// recently.
#[test]
fn all_four_real_signals_blend_together_and_are_all_named_in_the_evidence() {
    let (dir, monitor, token) = setup();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let engine = ContextEngine::new(graph.clone());

    let anchor = graph
        .put_node(
            &monitor,
            &token,
            None,
            "repository",
            None,
            json!({"name": "repo"}),
        )
        .unwrap();
    let scope = Scope {
        intent_id: "i".into(),
        session_id: "s-blend".into(),
        mentions: Vec::new(),
        anchors: vec![anchor],
    };
    engine
        .assemble(&monitor, &token, &scope, Budget::default())
        .unwrap();
    engine.record_expertise_signal("s-blend", ExpertiseSignal::VocabularyComplexity(0.8));
    engine.record_expertise_signal(
        "s-blend",
        ExpertiseSignal::CapabilityTierReach(CapabilityTierReach::RawApi),
    );
    engine.record_expertise_signal(
        "s-blend",
        ExpertiseSignal::ErrorRecovery(ErrorRecoveryPattern::SelfCorrected),
    );

    let estimate = engine.current_expertise("s-blend", "general");
    assert_eq!(estimate.evidence.len(), 4, "got: {:?}", estimate.evidence);
    assert!(estimate.evidence[0].contains("working-set entries"));
    assert!(estimate.evidence[1].contains("vocabulary complexity"));
    assert!(estimate.evidence[2].contains("raw Capability directly"));
    assert!(estimate.evidence[3].contains("self-corrected"));
}
