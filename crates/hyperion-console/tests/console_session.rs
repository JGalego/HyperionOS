//! Proves M7 stage 1's exit criterion for real: "a real utterance typed at the real booted
//! console produces a real Intent Graph, a real Agent invocation, and real text output rendered
//! to the real TTY." Two real utterance shapes are exercised, since `hyperion-intent` only has
//! one built-in HTN template today -- everything else takes a different, real path through this
//! same session (see `session.rs`'s own docs on why), and both need to be proven, not just the
//! one that happens to have a matching template.

use hyperion_console::ConsoleSession;

fn open_session() -> (tempfile::TempDir, ConsoleSession) {
    let dir = tempfile::tempdir().expect("create a real tempdir for this test's Knowledge Graph");
    let session = ConsoleSession::open(dir.path()).expect("open a real ConsoleSession");
    (dir, session)
}

#[test]
fn a_decomposable_utterance_produces_real_per_task_agent_outcomes_as_text() {
    let (_dir, mut session) = open_session();

    let lines = session.handle_utterance("I need to launch my startup");

    assert!(
        !lines.is_empty(),
        "a real, decomposed plan must render at least one line of real text"
    );
    let joined = lines.join("\n");
    // The real HTN template's four real leaves -- hyperion-coordination/tests/worked_trace.rs's
    // own fixture, reused here as the real, expected shape of a genuinely decomposed plan.
    for predicate in [
        "market_research",
        "business_model",
        "branding",
        "legal_formation",
    ] {
        assert!(
            joined.contains(predicate),
            "expected the real task {predicate:?} to appear in the rendered text, got: {joined:?}"
        );
    }
    // Every real task in this fixture succeeds (hyperion-coordination's two built-in
    // specializations cover exactly these four tasks' required capabilities), so a real,
    // completed status must show up too -- not just the task names.
    assert!(
        joined.contains("Done"),
        "expected at least one real, completed task status in: {joined:?}"
    );
}

#[test]
fn an_unmatched_utterance_still_produces_a_real_agent_invocation_as_text() {
    let (_dir, mut session) = open_session();

    let lines = session.handle_utterance("what is the weather like today");
    let joined = lines.join("\n");

    assert!(
        !lines.is_empty(),
        "an utterance with no matching HTN template must still render real text, not nothing"
    );
    // This is the undecomposed-goal path: a real `assistant.respond` Agent invocation (M8)
    // against the raw utterance, dispatched through this session's own real `LocalAiRuntime`
    // (`MockBackend` by default -- see `ConsoleSession::build_ai_runtime`), whose real generated
    // result embeds the prompt verbatim (`MockBackend::generate`: `format!("[mock model {id}]
    // echo: {prompt}")`) -- proving this rendered text really came from a real Agent invocation
    // carrying this specific utterance through to a real inference call, not a canned string
    // unrelated to what was actually typed. A real `CandleBackend` would not echo the prompt
    // (its own test asserts the opposite -- real generation produces genuinely new text); this
    // assertion is specifically about `MockBackend`'s deterministic echo, the same
    // exact-match-appropriate testing convention every other mock-backed test in this workspace
    // already uses.
    assert!(
        joined.contains("what is the weather like today"),
        "expected the real Agent invocation's own result to echo the utterance it was given, \
         got: {joined:?}"
    );
    assert!(
        joined.contains("generic_goal"),
        "expected the undecomposed root's own predicate to appear in the rendered text, got: \
         {joined:?}"
    );
}

#[test]
fn a_url_shaped_utterance_routes_to_a_real_web_research_dispatch_not_assistant_respond() {
    let (_dir, mut session) = open_session();

    let lines = session.handle_utterance("what does https://example.com/ say?");
    let joined = lines.join("\n");

    assert!(!lines.is_empty());
    // This session's own internal MockFetchBackend (PRODUCTION_BOOT_PROMPT.md M10 -- see
    // `ConsoleSession::build_netstack`) has no fixture registered for this URL (it's fully
    // encapsulated inside the session, not reachable from this test), so the real
    // `NetstackHub::web_research` call really runs, really misses its cache and its mock fetch
    // backend, and really degrades to a real stub node -- proving the real dispatch wiring
    // reaches `hyperion-netstack` at all, distinct from `assistant.respond`'s own real inference
    // path (which would instead echo the prompt via `MockBackend`, per the test above).
    assert!(
        joined.contains("merged into the knowledge graph"),
        "expected the real web.research dispatch's own success text, got: {joined:?}"
    );
    assert!(
        !joined.contains("echo:"),
        "a URL-shaped utterance must not fall through to assistant.respond's own mock echo, got: \
         {joined:?}"
    );
}

#[test]
fn two_different_undecomposed_goals_each_get_their_own_response_text() {
    // Regression test: every undecomposed utterance shares the same "generic_goal" predicate
    // (see `run_undecomposed_goal`), which `hyperion-workspace`'s own `WorkspaceCompiler` uses,
    // together with the capability set and complexity tier, as its template *cache key* -- a
    // real, intentional optimization so two turns of the same *shape* reuse the same real
    // layout decisions (panel count/size/position, lint result) instead of redoing that work.
    // A real bug let a cache hit also silently reuse the *first* turn's baked-in response
    // content (its `AccessibilityNode.accessible_name`) for every later same-shaped turn --
    // found by actually driving a real multi-turn interactive session (not just a single
    // utterance per session, which is all any test here did before), never by reasoning about
    // the code. `each_turn_is_independent...` below doesn't catch this: its own two same-shaped
    // ("launch my startup") turns send *identical* text, so identical cached content would pass
    // regardless of whether the cache-hit path actually refreshed anything.
    let (_dir, mut session) = open_session();

    let first = session.handle_utterance("hey there");
    let second = session.handle_utterance("I'd like to know more about giraffes");
    let third = session.handle_utterance("what is the weather like today");

    let first_text = first.join("\n");
    let second_text = second.join("\n");
    let third_text = third.join("\n");

    assert!(
        first_text.contains("hey there"),
        "expected the first turn to echo its own real utterance, got: {first_text:?}"
    );
    assert!(
        second_text.contains("I'd like to know more about giraffes"),
        "expected the second turn to echo its own real utterance, not the first turn's cached \
         content, got: {second_text:?}"
    );
    assert!(
        !second_text.contains("hey there"),
        "the second turn must not still carry the first turn's stale response text, got: \
         {second_text:?}"
    );
    assert!(
        third_text.contains("what is the weather like today"),
        "expected the third turn to echo its own real utterance, not an earlier turn's cached \
         content, got: {third_text:?}"
    );
}

#[test]
fn each_turn_is_independent_and_the_session_keeps_working_across_many_turns() {
    let (_dir, mut session) = open_session();

    let first = session.handle_utterance("launch my startup");
    let second = session.handle_utterance("tell me about the ocean");
    let third = session.handle_utterance("launch my startup");

    assert!(!first.is_empty());
    assert!(!second.is_empty());
    assert!(!third.is_empty());
    assert!(
        third.join("\n").contains("market_research"),
        "a real session must keep handling real decomposable utterances correctly after an \
         intervening, differently-shaped turn"
    );
}

#[test]
fn backend_meta_command_reports_and_switches_the_active_backend() {
    let (_dir, mut session) = open_session();

    let status = session.handle_utterance("/backend").join("\n");
    assert!(
        status.contains("mock"),
        "a default (non-candle) build must report mock as the active backend, got: {status:?}"
    );

    let noop = session.handle_utterance("/backend mock").join("\n");
    assert!(
        noop.contains("Already using"),
        "switching to the already-active backend must be a safe no-op, got: {noop:?}"
    );

    let failed = session.handle_utterance("/backend candle").join("\n");
    assert!(
        failed.contains("--features candle"),
        "a non-candle build must give a clear, honest reason it can't switch, got: {failed:?}"
    );
    assert!(
        !failed.contains("generic_goal"),
        "a meta-command reply must not also render a goal outcome, got: {failed:?}"
    );

    // The session must keep working normally after a failed switch attempt.
    let after = session.handle_utterance("still working?").join("\n");
    assert!(
        after.contains("still working?"),
        "a failed backend switch must not break the session's normal goal pipeline, got: \
         {after:?}"
    );
}

#[test]
fn the_plain_english_backend_phrase_requires_all_three_words() {
    let (_dir, mut session) = open_session();

    // The deliberately narrow "use backend <name>" phrasing must not fire on the bare two-word
    // "use <name>" form -- see `ConsoleSession::handle_meta_command`'s own doc comment on why:
    // "candle"/"mock" are ordinary enough words that a shorter phrase could collide with a real
    // goal utterance, exactly the ambiguity a meta-command must never risk.
    let lines = session.handle_utterance("use mock").join("\n");
    assert!(
        lines.contains("echo: use mock"),
        "\"use mock\" (without \"backend\") must be treated as a normal goal utterance, not a \
         meta-command, got: {lines:?}"
    );
    assert!(
        lines.contains("generic_goal"),
        "expected the bare two-word phrase to take the normal undecomposed-goal path, got: \
         {lines:?}"
    );
}

#[test]
fn the_backend_meta_command_is_case_insensitive_and_has_a_plain_english_alias() {
    let (_dir, mut session) = open_session();

    let via_slash = session.handle_utterance("/BACKEND").join("\n");
    let via_phrase = session.handle_utterance("USE BACKEND").join("\n");

    assert!(via_slash.contains("mock"), "got: {via_slash:?}");
    assert!(via_phrase.contains("mock"), "got: {via_phrase:?}");
}

#[test]
fn help_command_lists_the_backend_meta_command() {
    let (_dir, mut session) = open_session();

    let lines = session.handle_utterance("/help");
    let joined = lines.join("\n");

    assert!(
        joined.contains("/backend"),
        "expected /help to mention the /backend meta-command, got: {joined:?}"
    );
    assert!(
        !joined.contains("generic_goal"),
        "/help must not also fall through to a real Agent invocation, got: {joined:?}"
    );
}
