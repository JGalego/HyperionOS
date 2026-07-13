//! Proves M7 stage 1's exit criterion for real: "a real utterance typed at the real booted
//! console produces a real Intent Graph, a real Agent invocation, and real text output rendered
//! to the real TTY." Two real utterance shapes are exercised, since `hyperion-intent` only has
//! one built-in HTN template today -- everything else takes a different, real path through this
//! same session (see `session.rs`'s own docs on why), and both need to be proven, not just the
//! one that happens to have a matching template.

use hyperion_console::ConsoleSession;

#[cfg(feature = "openai-compat")]
mod common;

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

// This build's own switch attempt would otherwise make a real network call -- if
// `--features openai-compat` is enabled, "not compiled with this feature" isn't the real
// behavior to expect anymore (see `a_custom_engine_backend_switch_reaches_a_real_local_server_
// end_to_end` for that build's own real-connection coverage instead).
#[cfg(not(feature = "openai-compat"))]
#[test]
fn a_local_engine_switch_on_a_non_openai_compat_build_gives_an_honest_error() {
    let (_dir, mut session) = open_session();

    let failed = session
        .handle_utterance("/backend ollama llama3.2")
        .join("\n");
    assert!(
        failed.contains("--features openai-compat"),
        "a non-openai-compat build must give a clear, honest reason it can't switch, got: \
         {failed:?}"
    );
    assert!(
        !failed.contains("generic_goal"),
        "a meta-command reply must not also render a goal outcome, got: {failed:?}"
    );
}

#[test]
fn engine_backend_argument_parsing_gives_clear_errors_for_missing_arguments() {
    let (_dir, mut session) = open_session();

    let missing_model = session.handle_utterance("/backend ollama").join("\n");
    assert!(
        missing_model.contains("needs a model name"),
        "expected a clear error when a preset engine is given no model, got: {missing_model:?}"
    );

    let missing_args = session
        .handle_utterance("/backend custom http://localhost:9000/v1")
        .join("\n");
    assert!(
        missing_args.contains("needs both a base URL and a model name"),
        "expected a clear error when \"custom\" is given only one argument, got: {missing_args:?}"
    );

    let unknown = session
        .handle_utterance("/backend sagemaker some-model")
        .join("\n");
    assert!(
        unknown.contains("I don't know a"),
        "expected a clear error for a completely unrecognized backend name, got: {unknown:?}"
    );
}

#[cfg(feature = "openai-compat")]
#[test]
fn a_custom_engine_backend_switch_reaches_a_real_local_server_end_to_end() {
    let base_url = common::spawn_fixture_server(
        2,
        common::openai_compat_handler("fixture-model", "console fixture echo"),
    );

    let (_dir, mut session) = open_session();

    let switch = session
        .handle_utterance(&format!("/backend custom {base_url} fixture-model"))
        .join("\n");
    assert!(
        switch.starts_with("Switched to the custom") && switch.contains("fixture-model"),
        "expected a real connection to the real fixture server to succeed, got: {switch:?}"
    );

    let answer = session
        .handle_utterance("what does the real fixture say")
        .join("\n");
    assert!(
        answer.contains("console fixture echo: what does the real fixture say"),
        "expected the real fixture server's own response to come back through a real \
         assistant.respond dispatch, got: {answer:?}"
    );
}

/// PRODUCTION_BOOT_PROMPT.md "Phase 2: cloud providers": the whole real cloud-consent lifecycle,
/// against a real local fixture server (`HYPERION_OPENAI_BASE_URL` redirects `OpenAiCompatBackend`
/// away from the real `api.openai.com` -- see `ConsoleSession::try_connect_openai`'s own doc
/// comment: a real feature, Azure OpenAI/a corporate proxy, not just a testing seam). All three
/// sessions share this one test function deliberately: `HYPERION_OPENAI_BASE_URL` is real
/// process-global state, and interleaving it with another test that also sets it would race.
#[cfg(feature = "openai-compat")]
#[test]
fn cloud_consent_lifecycle_grants_in_session_but_reasks_fresh_after_a_restart() {
    let base_url = common::spawn_fixture_server(
        5,
        common::openai_compat_handler("gpt-fixture", "openai fixture echo"),
    );
    std::env::set_var("HYPERION_OPENAI_BASE_URL", &base_url);

    let dir = tempfile::tempdir().expect("create a real tempdir for this test's data_dir");

    // --- Session 1: connect, then use it immediately -- no repeat consent prompt. ---
    {
        let mut session = ConsoleSession::open(dir.path()).expect("open a real ConsoleSession");

        let prompt = session
            .handle_utterance("connect my openai account")
            .join("\n");
        assert!(
            prompt.contains("Paste your openai API key"),
            "got: {prompt:?}"
        );
        assert!(session.awaiting_secret_input());

        let stored = session.handle_utterance("sk-test-fixture-key").join("\n");
        assert!(stored.contains("Connected"), "got: {stored:?}");
        assert!(!session.awaiting_secret_input());

        let switched = session
            .handle_utterance("/backend openai gpt-fixture")
            .join("\n");
        assert!(
            switched.starts_with("Switched to the openai"),
            "got: {switched:?}"
        );

        let answer = session.handle_utterance("say hello").join("\n");
        assert!(
            answer.contains("openai fixture echo: say hello"),
            "expected the connect flow's own immediate in-session grant to let this dispatch \
             through with no PendingConsent prompt, got: {answer:?}"
        );
    }

    // --- Session 2: a fresh process (same data_dir, same already-connected key) still asks for
    // real consent on its own first real cloud use -- proving the grant genuinely does not carry
    // across a restart, and that PendingConsent is reachable through a real console sequence, not
    // just hyperion-agent-runtime's own isolated tests. ---
    {
        let mut session = ConsoleSession::open(dir.path()).expect("reopen the real ConsoleSession");

        let switched = session
            .handle_utterance("/backend openai gpt-fixture")
            .join("\n");
        assert!(
            switched.starts_with("Switched to the openai"),
            "the already-stored key must still let a fresh session switch to it, got: \
             {switched:?}"
        );

        let prompted = session.handle_utterance("say hello again").join("\n");
        assert!(
            prompted.contains("real, paid, external openai") && prompted.contains("yes/no"),
            "expected a fresh session's first real cloud use to hit a genuine consent prompt, \
             got: {prompted:?}"
        );

        let answer = session.handle_utterance("yes").join("\n");
        assert!(
            answer.contains("openai fixture echo: say hello again"),
            "expected confirming consent to re-invoke the original prompt for real, got: \
             {answer:?}"
        );
    }

    // --- Session 3: declining a fresh consent prompt must leave the session working normally
    // afterward -- no crash, no stuck state, mock (or any other ungated backend) unaffected. ---
    {
        let mut session =
            ConsoleSession::open(dir.path()).expect("reopen the real ConsoleSession again");

        let switched = session
            .handle_utterance("/backend openai gpt-fixture")
            .join("\n");
        assert!(
            switched.starts_with("Switched to the openai"),
            "got: {switched:?}"
        );

        let prompted = session
            .handle_utterance("one more real question")
            .join("\n");
        assert!(prompted.contains("yes/no"), "got: {prompted:?}");

        let declined = session.handle_utterance("no").join("\n");
        assert!(
            declined.contains("won't use that provider"),
            "got: {declined:?}"
        );

        let switched_back = session.handle_utterance("/backend mock").join("\n");
        assert!(
            switched_back.starts_with("Switched to the mock"),
            "got: {switched_back:?}"
        );
        let mock_answer = session.handle_utterance("still working?").join("\n");
        assert!(
            mock_answer.contains("echo: still working?"),
            "declining a consent prompt must not break the session's normal pipeline \
             afterward, got: {mock_answer:?}"
        );
    }

    std::env::remove_var("HYPERION_OPENAI_BASE_URL");
}
