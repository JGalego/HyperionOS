use hyperion_shell::{EmbeddedSession, IntentSink};

#[test]
fn an_undecomposed_goal_compiles_a_renderable_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = EmbeddedSession::open(dir.path()).expect("open a real session");

    let outcome = session.handle_utterance("help me plan a weekend trip");

    let graph = outcome
        .graph
        .expect("an ordinary goal always compiles a workspace");
    assert!(!graph.panels.is_empty());
    let tree = outcome
        .tree
        .expect("a compiled workspace always carries a tree");
    assert!(!tree.nodes.is_empty());
    assert!(!outcome.narration.is_empty());
}

#[test]
fn a_decomposed_plan_renders_one_panel_per_task() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = EmbeddedSession::open(dir.path()).expect("open a real session");

    // The one built-in HTN template this workspace ships (matches
    // hyperion-coordination/tests/worked_trace.rs's own utterance).
    let outcome = session.handle_utterance("launch my startup");

    let graph = outcome
        .graph
        .expect("a decomposed plan always compiles a workspace");
    assert!(
        graph.panels.len() > 1,
        "a multi-task plan should render more than one panel, got {}",
        graph.panels.len()
    );
}
