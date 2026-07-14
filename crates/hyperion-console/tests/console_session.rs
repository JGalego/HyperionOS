//! Proves M7 stage 1's exit criterion for real: "a real utterance typed at the real booted
//! console produces a real Intent Graph, a real Agent invocation, and real text output rendered
//! to the real TTY." Two real utterance shapes are exercised, since `hyperion-intent` only has
//! one built-in HTN template today -- everything else takes a different, real path through this
//! same session (see `session.rs`'s own docs on why), and both need to be proven, not just the
//! one that happens to have a matching template.

use hyperion_console::{ConsoleSession, TaskProgress};

#[cfg(feature = "openai-compat")]
mod common;

fn open_session() -> (tempfile::TempDir, ConsoleSession) {
    let dir = tempfile::tempdir().expect("create a real tempdir for this test's Knowledge Graph");
    let session = ConsoleSession::open(dir.path()).expect("open a real ConsoleSession");
    (dir, session)
}

/// Regression test: `ConsoleSession::open` used to crash with a raw "No such file or directory"
/// WAL error the very first time it was ever pointed at a data directory that didn't already
/// exist -- i.e. a genuinely fresh install, never created for the caller and only ever assumed
/// to be there already.
#[test]
fn open_creates_a_genuinely_fresh_data_directory_rather_than_failing() {
    let parent = tempfile::tempdir().expect("create a real tempdir");
    let never_created = parent.path().join("fresh-install-data-dir");
    assert!(
        !never_created.exists(),
        "sanity: this path must not exist yet for this test to mean anything"
    );

    let session = ConsoleSession::open(&never_created);
    assert!(
        session.is_ok(),
        "opening a session against a real, never-before-seen data directory must succeed, not \
         crash, got: {:?}",
        session.err()
    );
    assert!(
        never_created.is_dir(),
        "the data directory must actually have been created"
    );
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

/// A real, previously-shipped bug this regression-tests: every one of these four tasks used to
/// render as just its own status word ("Done"), because `hyperion-coordination::allocate`
/// discarded a real capability's own real output before this console ever saw it -- see that
/// crate's own doc comment on the "launch my startup produces zero real content" gap. `document.
/// draft`/`web.search` now dispatch through a real `LocalAiRuntime::infer` call
/// (`hyperion-agent-runtime`'s own fix), and `MockBackend` deterministically echoes the whole
/// prompt built from this task's own predicate and the real root utterance -- so the rendered
/// text must carry real, task-specific substance, not just "Done."
#[test]
fn a_decomposed_plans_own_tasks_render_real_generated_content_not_just_a_status_word() {
    let (_dir, mut session) = open_session();

    let lines = session.handle_utterance("I need to launch my startup");
    let joined = lines.join("\n");

    assert!(
        joined.contains("Done -- [mock model"),
        "expected a completed task's real, prompt-driven content alongside its status, not a \
         bare status word, got: {joined:?}"
    );
    assert!(
        joined.contains("I need to launch my startup"),
        "expected the real root utterance (passed as this session's own \"goal\" arg) to show \
         up in the real, generated text, got: {joined:?}"
    );
    assert!(
        joined.contains("(see \"/result market_research\" for the full text)"),
        "expected a short preview plus a concrete pointer to the full text, not the whole \
         (potentially several-paragraph, for a real cloud model) result dumped inline, got: \
         {joined:?}"
    );

    // The full text -- including market_research's own honesty caveat -- isn't lost, just not
    // printed inline: /result finds it directly, via the real "produced" edge `allocate` records,
    // not a text search (a real model's own prose doesn't always repeat a task's own predicate
    // verbatim -- see `GraphExplorer::result`'s own doc comment).
    let result = session
        .handle_utterance("/result market_research")
        .join("\n");
    assert!(
        result.contains("AI-generated research notes, not a live web search"),
        "market_research's own real result must carry its honesty caveat, in full, when asked \
         about directly via /result, got: {result:?}"
    );
}

/// A real, previously-shipped UX gap this regression-tests: the console used to print nothing
/// at all while a decomposed multi-task plan worked through several real capability dispatches,
/// one tick at a time -- only the *final*, fully-converged result ever appeared, and there was no
/// way to tell a task had even *started* before it finished. `Starting` must fire once per tick,
/// naming every task about to run concurrently, *before* that tick's own blocking dispatch; `Done`
/// fires once per task after.
#[test]
fn a_decomposed_plans_progress_callback_fires_starting_then_done_once_per_tick() {
    let (_dir, mut session) = open_session();
    let mut starting: Vec<Vec<String>> = Vec::new();
    let mut done: Vec<String> = Vec::new();

    let final_lines =
        session.handle_utterance_with_progress("I need to launch my startup", &mut |event| {
            match event {
                TaskProgress::Starting(names) => starting.push(names),
                TaskProgress::Done(line) => done.push(line),
            }
        });

    // Three ticks: market_research alone; business_model + branding together; legal_formation
    // alone -- see hyperion-intent/src/templates.rs's own dependency shape.
    assert_eq!(
        starting.len(),
        3,
        "one Starting event per tick, got: {starting:?}"
    );
    assert_eq!(starting[0], vec!["market_research".to_string()]);
    let mut tick_two = starting[1].clone();
    tick_two.sort();
    assert_eq!(
        tick_two,
        vec!["branding".to_string(), "business_model".to_string()],
        "business_model and branding become ready together, in the same Starting event"
    );
    assert_eq!(starting[2], vec!["legal_formation".to_string()]);

    assert_eq!(done.len(), 4, "one Done event per task, got: {done:?}");
    for predicate in [
        "market_research",
        "business_model",
        "branding",
        "legal_formation",
    ] {
        assert!(
            done.iter().any(|line| line.contains(predicate)),
            "got: {done:?}"
        );
    }
    assert!(
        done.iter().all(|line| line.contains("Done")),
        "every real task in this fixture succeeds, so every Done event must say so, got: {done:?}"
    );

    // The plain `handle_utterance` path (used everywhere else in this file) must still return
    // the exact same final result regardless of whether a caller also wanted live progress.
    let plain_lines = {
        let (_dir2, mut plain_session) = open_session();
        plain_session.handle_utterance("I need to launch my startup")
    };
    assert_eq!(
        final_lines.len(),
        plain_lines.len(),
        "the progress callback must not change the shape of the final rendered result"
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
    // The second turn now legitimately quotes "hey there" as real recent-conversation context
    // (see `ConsoleSession::prompt_with_recent_history`) -- a plain substring check can no
    // longer distinguish that from the old stale-cache bleed, so this checks for the bug's own
    // actual signature instead: the *whole* response being a stale copy of the first turn's,
    // not just a legitimate partial quote of it.
    assert_ne!(
        second_text, first_text,
        "the second turn's response must be its own, not a stale copy of the first turn's, got: \
         {second_text:?}"
    );
    assert!(
        third_text.contains("what is the weather like today"),
        "expected the third turn to echo its own real utterance, not an earlier turn's cached \
         content, got: {third_text:?}"
    );
}

/// Regression test: `ConsoleSession` used to mint a brand-new, unique session id for every
/// single turn and pass *that* to `IntentEngine::handle_utterance` -- so its real working-memory
/// turn buffer (and `hyperion-context`'s own working-set hysteresis, keyed the same way) never
/// accumulated more than one turn's worth of history before being silently discarded and
/// recreated empty on the very next turn. A real follow-up question in the same conversation got
/// no benefit from any of it. Proven here via the one real, externally observable signal this
/// crate's own default (mock) backend gives: `MockBackend::generate` echoes its prompt verbatim,
/// so a real "recent conversation" prefix genuinely reaching the model shows up directly in the
/// rendered response text.
#[test]
fn a_followup_utterance_carries_real_conversation_history_into_its_own_prompt() {
    let (_dir, mut session) = open_session();

    let first = session.handle_utterance("my name is Alex");
    let second = session.handle_utterance("what is my name");

    let first_text = first.join("\n");
    let second_text = second.join("\n");
    assert!(
        first_text.contains("my name is Alex"),
        "the first turn (nothing prior to recap) must be unchanged: bare utterance, no prefix, \
         got: {first_text:?}"
    );
    assert!(
        second_text.contains("my name is Alex") && second_text.contains("what is my name"),
        "the second turn's real prompt must carry the first turn's utterance as recent \
         conversation *and* still ask its own real question, got: {second_text:?}"
    );

    // A brand-new, separate session must never see the first session's history -- this is real
    // per-session state, not a global leak across every `ConsoleSession`.
    let (_dir2, mut other_session) = open_session();
    let unrelated = other_session.handle_utterance("what is my name").join("\n");
    assert!(
        !unrelated.contains("my name is Alex"),
        "a fresh, unrelated session must not carry another session's real conversation \
         history, got: {unrelated:?}"
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
        joined.contains("/recall"),
        "expected /help to mention the /recall meta-command, got: {joined:?}"
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

/// A real bug report motivated this: a user's real OpenAI key kept failing with a real 401 even
/// though they believed it was correct, with no way to verify what actually got stored short of
/// exposing the real secret. Doesn't need `openai-compat` -- this only exercises the storage/
/// confirmation step, never a real connection.
#[test]
fn connect_strips_stray_control_characters_and_shows_a_masked_preview_not_the_real_secret() {
    let (_dir, mut session) = open_session();

    session.handle_utterance("connect my openai account");
    // A real, observed terminal-artifact class: a stray embedded control character (here, a
    // literal NUL) that `.trim()` alone doesn't catch, since it only trims *leading/trailing*
    // Unicode whitespace, not a character sitting in the middle of the line.
    let dirty_key = "sk-real\u{0}keyvalue1234";
    let stored = session.handle_utterance(dirty_key).join("\n");

    assert!(stored.contains("Connected"), "got: {stored:?}");
    assert!(
        !stored.contains("sk-realkeyvalue1234") && !stored.contains(dirty_key),
        "the confirmation must never show the real secret (cleaned or as-typed), got: {stored:?}"
    );
    assert!(
        stored.contains("sk-r...1234"),
        "expected a masked preview reflecting the real, control-character-stripped key, got: \
         {stored:?}"
    );
}

/// `/recall`/`/why`/`/related` -- exploring the real, live Knowledge Graph this session's own
/// Intent Engine writes to on every utterance. Every assertion here is against real data these
/// tests themselves caused to be recorded, not fixtures -- the same "prove non-vacuously"
/// discipline this file already applies to the rest of the pipeline.
mod graph_exploration {
    use super::open_session;

    #[test]
    fn recall_with_nothing_recorded_yet_says_so() {
        let (_dir, mut session) = open_session();

        let lines = session.handle_utterance("/recall").join("\n");
        assert!(
            lines.contains("anything recorded yet"),
            "a brand-new session's graph really is empty, got: {lines:?}"
        );
    }

    #[test]
    fn recall_finds_a_recorded_intent_by_its_own_words() {
        let (_dir, mut session) = open_session();
        session.handle_utterance("what is the weather like today");

        let lines = session.handle_utterance("/recall weather").join("\n");
        assert!(
            lines.contains("[1] you asked: \"what is the weather like today\""),
            "expected the real utterance just recorded to come back, got: {lines:?}"
        );
    }

    #[test]
    fn recall_bare_lists_everything_recorded_so_far() {
        let (_dir, mut session) = open_session();
        session.handle_utterance("what is the weather like today");

        let lines = session.handle_utterance("/recall").join("\n");
        assert!(
            lines.contains("you asked: \"what is the weather like today\""),
            "a bare /recall with no search text should still surface what's recorded, got: \
             {lines:?}"
        );
    }

    #[test]
    fn recall_with_no_match_says_so() {
        let (_dir, mut session) = open_session();
        session.handle_utterance("what is the weather like today");

        let lines = session
            .handle_utterance("/recall zzz_nothing_matches_this_zzz")
            .join("\n");
        assert!(
            lines.contains("I don't have anything recorded about"),
            "got: {lines:?}"
        );
    }

    #[test]
    fn why_explains_an_isolated_intent_with_no_connections() {
        let (_dir, mut session) = open_session();
        session.handle_utterance("what is the weather like today");
        session.handle_utterance("/recall weather");

        let lines = session.handle_utterance("/why 1").join("\n");
        assert!(
            lines.contains("[1] is something you asked, recorded"),
            "got: {lines:?}"
        );
        assert!(
            lines.contains("isn't connected to anything else yet"),
            "an undecomposed goal creates exactly one real node with no real edges, got: \
             {lines:?}"
        );
    }

    #[test]
    fn related_reveals_real_dependency_edges_between_decomposed_tasks() {
        let (_dir, mut session) = open_session();
        // The real, built-in HTN template (hyperion-intent/src/templates.rs): business_model and
        // branding both really depend on market_research, so real `depends_on` edges connect them
        // in the real graph -- not just shared membership in one Intent's `children` list.
        session.handle_utterance("I need to launch my startup");

        // "planned task: market_research" (not the bare predicate) uniquely matches the task
        // node itself -- `hyperion-coordination::allocate` now also writes a real "task_result"
        // node holding market_research's own real, generated content, which would otherwise
        // *also* match a bare "market_research" search (its own prompt names the task).
        let recalled = session
            .handle_utterance("/recall planned task: market_research")
            .join("\n");
        assert!(
            recalled.contains("[1] a planned task: market_research"),
            "got: {recalled:?}"
        );

        let related = session.handle_utterance("/related 1").join("\n");
        assert!(related.contains("business_model"), "got: {related:?}");
        assert!(related.contains("branding"), "got: {related:?}");
    }

    /// Regression test for a real bug a live manual check caught: an earlier `why()` classified
    /// every "intent"-typed node as "something you asked," including a decomposed goal's own
    /// child tasks (which carry no utterance of their own -- nobody actually said
    /// "market_research").
    #[test]
    fn why_distinguishes_a_planned_subtask_from_something_actually_said() {
        let (_dir, mut session) = open_session();
        session.handle_utterance("I need to launch my startup");
        session.handle_utterance("/recall planned task: market_research");

        let lines = session.handle_utterance("/why 1").join("\n");
        assert!(
            lines.contains("[1] is a planned task, recorded"),
            "got: {lines:?}"
        );
    }

    #[test]
    fn why_reports_a_real_connection_count_for_a_task_with_edges() {
        let (_dir, mut session) = open_session();
        session.handle_utterance("I need to launch my startup");
        session.handle_utterance("/recall planned task: market_research");

        let lines = session.handle_utterance("/why 1").join("\n");
        assert!(
            lines.contains("connected to 3 other things"),
            "market_research is the target of two real depends_on edges (from business_model \
             and branding) plus one real \"produced\" edge to its own real task_result node \
             (hyperion-coordination::allocate no longer discards a capability's real output), \
             got: {lines:?}"
        );
    }

    #[test]
    fn related_results_renumber_so_a_second_related_call_can_drill_further() {
        let (_dir, mut session) = open_session();
        session.handle_utterance("I need to launch my startup");
        // Not a bare "market_research" search: that would also match the real "task_result"
        // node `hyperion-coordination::allocate` now writes (its own generated text names the
        // task), and this test needs [1] to be the task itself.
        session.handle_utterance("/recall planned task: market_research");

        let first_hop = session.handle_utterance("/related 1").join("\n");
        assert!(
            first_hop.contains("business_model") && first_hop.contains("branding"),
            "got: {first_hop:?}"
        );

        // `/related 1` just re-numbered its own output -- a second `/related 1` must resolve
        // against *that* fresh list, not silently reuse the previous one or error out.
        let second_hop = session.handle_utterance("/related 1").join("\n");
        assert!(
            !second_hop.contains("don't have a"),
            "a freshly re-numbered [1] must resolve to a real node, got: {second_hop:?}"
        );
    }

    #[test]
    fn why_and_related_reject_an_unknown_reference_number() {
        let (_dir, mut session) = open_session();

        let why = session.handle_utterance("/why 1").join("\n");
        assert!(why.contains("don't have a \"[1]\""), "got: {why:?}");

        let related = session.handle_utterance("/related 3").join("\n");
        assert!(related.contains("don't have a \"[3]\""), "got: {related:?}");
    }

    #[test]
    fn why_and_related_reject_a_non_numeric_argument() {
        let (_dir, mut session) = open_session();

        let why = session.handle_utterance("/why abc").join("\n");
        assert!(why.contains("needs a result number"), "got: {why:?}");

        let related = session.handle_utterance("/related abc").join("\n");
        assert!(
            related.contains("needs a result number"),
            "got: {related:?}"
        );
    }

    #[test]
    fn result_finds_a_tasks_real_output_directly_by_name_no_numbered_detour() {
        let (_dir, mut session) = open_session();
        session.handle_utterance("I need to launch my startup");

        let result = session
            .handle_utterance("/result business_model")
            .join("\n");
        assert!(
            result.contains("\"business_model\"'s real result, recorded"),
            "got: {result:?}"
        );
        assert!(
            result.contains("Draft a concise, practical business_model"),
            "expected the real, generated text itself, got: {result:?}"
        );
    }

    #[test]
    fn result_is_case_insensitive_on_the_task_name() {
        let (_dir, mut session) = open_session();
        session.handle_utterance("I need to launch my startup");

        let result = session
            .handle_utterance("/result Business_Model")
            .join("\n");
        assert!(
            result.contains("Draft a concise, practical business_model"),
            "got: {result:?}"
        );
    }

    #[test]
    fn result_reports_honestly_on_an_unknown_task_name() {
        let (_dir, mut session) = open_session();
        session.handle_utterance("I need to launch my startup");

        let result = session
            .handle_utterance("/result not_a_real_task")
            .join("\n");
        assert!(
            result.contains("I don't have a task called \"not_a_real_task\""),
            "got: {result:?}"
        );
    }

    #[test]
    fn result_rejects_a_bare_argument() {
        let (_dir, mut session) = open_session();

        let result = session.handle_utterance("/result").join("\n");
        assert!(result.contains("needs a task name"), "got: {result:?}");
    }
}

/// `/redo <task> <extra instructions>` -- the real "steer this task with more information" verb,
/// backed by `hyperion_coordination::CoordinationSession::amend_task`. Every assertion here drives
/// a real decomposed plan through a full run first, then redoes one of its own real tasks, so the
/// regenerated content is genuinely new output from the same `MockBackend` echo, not a fixture.
mod redo_and_steer {
    use super::open_session;
    use hyperion_console::TaskProgress;

    #[test]
    fn redo_with_no_prior_plan_says_so() {
        let (_dir, mut session) = open_session();

        let result = session
            .handle_utterance("/redo market_research focus on Europe")
            .join("\n");
        assert!(result.contains("no plan to redo yet"), "got: {result:?}");
    }

    #[test]
    fn redo_rejects_a_bare_task_name_argument() {
        let (_dir, mut session) = open_session();
        session.handle_utterance("I need to launch my startup");

        let result = session.handle_utterance("/redo").join("\n");
        assert!(result.contains("needs a task name"), "got: {result:?}");
    }

    #[test]
    fn redo_reports_honestly_on_an_unknown_task_name() {
        let (_dir, mut session) = open_session();
        session.handle_utterance("I need to launch my startup");

        let result = session
            .handle_utterance("/redo not_a_real_task with more detail")
            .join("\n");
        assert!(
            result.contains("I don't have a task called \"not_a_real_task\""),
            "got: {result:?}"
        );
    }

    /// The real, motivating scenario: a completed task's own real result gets regenerated with
    /// the user's new steering text folded into the real prompt -- not just reset to `Unassigned`
    /// and left there.
    #[test]
    fn redo_regenerates_a_completed_tasks_result_with_the_real_extra_context() {
        let (_dir, mut session) = open_session();
        session.handle_utterance("I need to launch my startup");

        let redone = session
            .handle_utterance("/redo market_research focus on the European market only")
            .join("\n");
        assert!(
            redone.contains("Done"),
            "the redone task must finish Done again, got: {redone:?}"
        );

        let result = session
            .handle_utterance("/result market_research")
            .join("\n");
        assert!(
            result.contains("focus on the European market only"),
            "expected the real steering text to show up in the real, regenerated result, got: \
             {result:?}"
        );
    }

    #[test]
    fn redo_is_case_insensitive_on_the_task_name() {
        let (_dir, mut session) = open_session();
        session.handle_utterance("I need to launch my startup");

        let redone = session
            .handle_utterance("/redo Market_Research focus on Asia")
            .join("\n");
        assert!(redone.contains("Done"), "got: {redone:?}");

        let result = session
            .handle_utterance("/result market_research")
            .join("\n");
        assert!(result.contains("focus on Asia"), "got: {result:?}");
    }

    /// Redoing never cascades automatically -- but the tasks that already depended on the old
    /// result must be named, so the user knows to redo them too if they want them updated.
    #[test]
    fn redo_warns_about_dependents_that_already_used_the_old_result() {
        let (_dir, mut session) = open_session();
        session.handle_utterance("I need to launch my startup");

        let redone = session
            .handle_utterance("/redo market_research focus on Europe")
            .join("\n");
        assert!(
            redone.contains("business_model") && redone.contains("branding"),
            "business_model and branding both directly depend on market_research and were \
             already Done, so both must be named as using the now-stale result, got: {redone:?}"
        );
        assert!(
            redone.contains("won't be redone automatically"),
            "got: {redone:?}"
        );
    }

    /// `/redo` re-drives the plan's own real ticks, so a caller watching for live progress (e.g.
    /// this console's own spinner) must still see it -- not just the plain `handle_utterance`
    /// path's silent final result.
    #[test]
    fn redo_fires_the_same_real_progress_events_as_the_plans_first_run() {
        let (_dir, mut session) = open_session();
        session.handle_utterance("I need to launch my startup");

        let mut starting: Vec<Vec<String>> = Vec::new();
        let mut done: Vec<String> = Vec::new();
        session.handle_utterance_with_progress(
            "/redo market_research focus on Europe",
            &mut |event| match event {
                TaskProgress::Starting(names) => starting.push(names),
                TaskProgress::Done(line) => done.push(line),
            },
        );

        assert_eq!(
            starting,
            vec![vec!["market_research".to_string()]],
            "got: {starting:?}"
        );
        assert_eq!(done.len(), 1, "got: {done:?}");
        assert!(done[0].contains("market_research"), "got: {done:?}");
    }
}
