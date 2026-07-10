//! docs/08-memory-engine.md §6/§7/§8: remember/query/recall round-trip
//! through a real Knowledge Graph, erase cascade, extraction frequency
//! gate, and the "no hidden writes" conformance shape (every write is
//! reachable from `query`).

use std::sync::Arc;

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
fn run_co_occurrence_pass_links_every_pair_of_objects_a_memory_record_names() {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let engine = MemoryEngine::new(graph.clone());

    let repo = graph
        .put_node(&monitor, &token, None, "repository", None, json!({}))
        .unwrap();
    let issue = graph
        .put_node(&monitor, &token, None, "issue", None, json!({}))
        .unwrap();
    let unrelated = graph
        .put_node(&monitor, &token, None, "document", None, json!({}))
        .unwrap();

    // One real memory record naming both repo and issue in its
    // provenance -- the real signal run_co_occurrence_pass sources from.
    engine
        .remember(
            &monitor,
            &token,
            MemoryTier::Episodic,
            json!({"summary": "discussed the flaky test in this issue"}),
            None,
            0.5,
            false,
            vec![repo, issue],
        )
        .unwrap();

    let edges_touched = engine.run_co_occurrence_pass(&monitor, &token).unwrap();
    assert_eq!(edges_touched, 1);

    let subgraph = graph.traverse(&monitor, &token, repo, None, 1).unwrap();
    let co_occurs = subgraph
        .edges
        .iter()
        .find(|(_, e)| e.predicate == "co-occurs-with" && e.target == issue);
    assert!(
        co_occurs.is_some(),
        "repo and issue must be linked by a real co-occurs-with edge"
    );

    let unrelated_edge = subgraph
        .edges
        .iter()
        .find(|(_, e)| e.target == unrelated || e.subject == unrelated);
    assert!(
        unrelated_edge.is_none(),
        "an object never named alongside repo must not be linked"
    );
}

#[test]
fn remember_and_query_round_trips_through_the_knowledge_graph() {
    let (_dir, monitor, token, engine) = setup();
    let id = engine
        .remember(
            &monitor,
            &token,
            MemoryTier::Episodic,
            json!({"summary": "helped with interview prep"}),
            None,
            0.5,
            false,
            Vec::new(),
        )
        .unwrap();

    let results = engine
        .query(&monitor, &token, &MemoryFilter::default())
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, id);
    assert_eq!(results[0].content["summary"], "helped with interview prep");
}

#[test]
fn remember_explicit_bypasses_decay_and_mirrors_to_long_term() {
    let (_dir, monitor, token, engine) = setup();
    let (semantic_id, long_term_id) = engine
        .remember_explicit(
            &monitor,
            &token,
            json!({"fact": "allergic to peanuts"}),
            None,
        )
        .unwrap();

    let semantic = engine
        .query(
            &monitor,
            &token,
            &MemoryFilter {
                tier: Some(MemoryTier::Semantic),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(semantic.len(), 1);
    assert_eq!(semantic[0].id, semantic_id);
    assert!(semantic[0].pinned);
    assert_eq!(semantic[0].decay_score, 1.0);

    let long_term = engine
        .query(
            &monitor,
            &token,
            &MemoryFilter {
                tier: Some(MemoryTier::LongTerm),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(long_term.len(), 1);
    assert_eq!(long_term[0].id, long_term_id);
}

#[test]
fn recall_ranks_by_embedding_similarity() {
    let (_dir, monitor, token, engine) = setup();
    let close = engine
        .remember(
            &monitor,
            &token,
            MemoryTier::Semantic,
            json!({"fact": "prefers dark mode"}),
            Some(vec![1.0, 0.0]),
            0.5,
            false,
            Vec::new(),
        )
        .unwrap();
    engine
        .remember(
            &monitor,
            &token,
            MemoryTier::Semantic,
            json!({"fact": "unrelated"}),
            Some(vec![0.0, 1.0]),
            0.5,
            false,
            Vec::new(),
        )
        .unwrap();

    let results = engine.recall(&monitor, &token, vec![1.0, 0.0], 1).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, close);
}

#[test]
fn edit_merges_the_patch_into_content() {
    let (_dir, monitor, token, engine) = setup();
    let id = engine
        .remember(
            &monitor,
            &token,
            MemoryTier::Semantic,
            json!({"fact": "works_at", "value": "Acme"}),
            None,
            0.5,
            false,
            Vec::new(),
        )
        .unwrap();

    let edited = engine
        .edit(&monitor, &token, id, json!({"value": "Critical Software"}))
        .unwrap();
    assert_eq!(edited.content["value"], "Critical Software");
    assert_eq!(
        edited.content["fact"], "works_at",
        "unpatched fields survive the merge"
    );
}

#[test]
fn erase_is_soft_delete_hidden_by_default_but_visible_with_include_erased() {
    let (_dir, monitor, token, engine) = setup();
    let id = engine
        .remember(
            &monitor,
            &token,
            MemoryTier::Episodic,
            json!({}),
            None,
            0.1,
            false,
            Vec::new(),
        )
        .unwrap();

    engine.erase(&monitor, &token, id, false).unwrap();

    assert!(engine
        .query(&monitor, &token, &MemoryFilter::default())
        .unwrap()
        .is_empty());
    let visible = engine
        .query(
            &monitor,
            &token,
            &MemoryFilter {
                include_erased: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(visible.len(), 1);
    assert!(visible[0].erased);
}

#[test]
fn erase_cascades_to_dependent_facts() {
    let (_dir, monitor, token, engine) = setup();
    let episode = engine
        .remember(
            &monitor,
            &token,
            MemoryTier::Episodic,
            json!({}),
            None,
            0.1,
            false,
            Vec::new(),
        )
        .unwrap();
    let fact = engine
        .remember(
            &monitor,
            &token,
            MemoryTier::Semantic,
            json!({"fact": "derived"}),
            None,
            0.5,
            false,
            vec![episode],
        )
        .unwrap();

    let receipt = engine.erase(&monitor, &token, episode, true).unwrap();
    assert_eq!(receipt.cascaded, vec![fact]);

    let visible = engine
        .query(
            &monitor,
            &token,
            &MemoryFilter {
                include_erased: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(visible.iter().all(|r| r.erased));
}

#[test]
fn extraction_promotes_only_after_the_frequency_gate() {
    let (_dir, monitor, token, engine) = setup();
    for _ in 0..2 {
        engine
            .remember(
                &monitor,
                &token,
                MemoryTier::Episodic,
                json!({"entity_key": "user", "fact": "prefers_dark_mode"}),
                None,
                0.1,
                false,
                Vec::new(),
            )
            .unwrap();
    }
    let receipt = engine.run_extraction_pass(&monitor, &token, 3).unwrap();
    assert!(
        receipt.promoted.is_empty(),
        "two occurrences must not clear a 3-occurrence gate"
    );

    engine
        .remember(
            &monitor,
            &token,
            MemoryTier::Episodic,
            json!({"entity_key": "user", "fact": "prefers_dark_mode"}),
            None,
            0.1,
            false,
            Vec::new(),
        )
        .unwrap();
    let receipt = engine.run_extraction_pass(&monitor, &token, 3).unwrap();
    assert_eq!(
        receipt.promoted.len(),
        1,
        "the third occurrence must clear the gate"
    );

    let semantic = engine
        .query(
            &monitor,
            &token,
            &MemoryFilter {
                tier: Some(MemoryTier::Semantic),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(semantic.len(), 1);
    assert_eq!(semantic[0].content["fact"], "prefers_dark_mode");
}

#[test]
fn decay_pass_promotes_a_frequently_recalled_important_record_but_not_a_fresh_unimportant_one() {
    let (_dir, monitor, token, engine) = setup();
    let important = engine
        .remember(
            &monitor,
            &token,
            MemoryTier::Semantic,
            json!({}),
            Some(vec![1.0, 0.0]),
            1.0,
            false,
            Vec::new(),
        )
        .unwrap();
    let unimportant = engine
        .remember(
            &monitor,
            &token,
            MemoryTier::Semantic,
            json!({}),
            None,
            0.0,
            false,
            Vec::new(),
        )
        .unwrap();

    // Repeated recall is what drives F(r) up in docs/08 §5.2's score —
    // a record that's never retrieved never accrues frequency credit.
    for _ in 0..30 {
        engine.recall(&monitor, &token, vec![1.0, 0.0], 1).unwrap();
    }

    engine.run_decay_pass(&monitor, &token).unwrap();

    let long_term = engine
        .query(
            &monitor,
            &token,
            &MemoryFilter {
                tier: Some(MemoryTier::LongTerm),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(
        long_term.iter().any(|r| r.provenance.contains(&important)),
        "the frequently-recalled, high-importance record should have been promoted to Long-Term"
    );
    assert!(
        !long_term
            .iter()
            .any(|r| r.provenance.contains(&unimportant)),
        "a fresh, never-recalled, zero-importance record must not be promoted"
    );
}
