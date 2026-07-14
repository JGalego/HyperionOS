# Hyperion Usage Scenarios — a living acceptance-test log

This document is the durable record of docs/41's Phase 2 ("model real usage") and Phase 3
("compare reality vs. expectations") work: realistic scenarios actually driven against the real,
compiled system, what was observed, and what happened as a result. It is meant to grow the same
way [35 — Testing Strategy](docs/35-testing-strategy.md)'s golden corpus grows — from real
sessions, not only hand-authored cases — except this file is the human-readable log; the durable,
machine-checked form of each finding below lives as a real regression test in the relevant crate
(linked per entry).

**Status: a starting set, not the "dozens" docs/41 asks for.** ~10 scenarios have been run so far,
against `hyperion-console` only (the one real, natively-buildable, non-GUI entry point this
sandbox can drive with piped stdin — see "How scenarios are run" below). `hyperion-shell` (a real
GUI needing a display) and full-system scenarios (package management, multi-user permissions,
process management, actual filesystem operations) have not been run at all yet and are named as
gaps in coverage, not silently treated as covered. Anyone picking this up should add scenarios in
the same format, run them for real, and append the result — do not add a hypothetical scenario
without actually running it, and do not mark a finding "fixed" without a regression test proving
it.

## How scenarios are run

```
cargo build -p hyperion-console --bin hyperion-console
HYPERION_CONSOLE_DATA_DIR=<a real, disposable directory> \
    printf '<utterance>\n<utterance>\n...' | ./target/debug/hyperion-console
```

This is a native, host build of the real binary (no cross-compilation, no QEMU) — the same
"native build, piped stdin" pattern the production boot roadmap's own M12 work established as the
fastest real debug loop. It defaults to `MockBackend` (no `--features candle`) and no cloud
credentials, i.e. exactly what anyone cloning this repo and running the console gets without extra
setup — deliberately the *worst-case* real default, not a cherry-picked best case.

## Scenario log

Each entry: persona/situation, the utterance(s) used, what was actually observed, and the
outcome. "Fixed" entries link the commit and the regression test that now proves it stays fixed.

### 1. Beginner, very first launch ever

**Utterance:** fresh, never-before-used data directory, then `help me plan a weekend trip`.

**Observed:** crashed immediately —
`storage error: WAL I/O error: No such file or directory (os error 2)`, a raw technical error.

**Finding:** `ConsoleSession::open` never created its own data directory, only assumed it already
existed.

**Status:** Fixed — `4071208`, regression test
`open_creates_a_genuinely_fresh_data_directory_rather_than_failing` in
`crates/hyperion-console/tests/console_session.rs`.

### 2. Beginner, plain request (directory now exists)

**Utterance:** `help me plan a weekend trip`

**Observed:** `status: generic_goal: done -- [mock model 1] echo: help me plan a weekend trip`

**Finding:** the `generic_goal` internal sentinel leaked into the rendered response, redundant
with the real (and legitimately accessibility-motivated) "status: " role announcement in front of
it.

**Status:** Fixed — `31229fe`. The real underlying UX gap this scenario also exposes — the
out-of-the-box default is `MockBackend`, whose `generate()` is a literal `echo:` of the prompt, so
*any* plain request looks like this without extra setup (`--features candle` or a connected cloud
account) — is named here as a real, open finding, not fixed: see "Open findings" below.

### 3. Power user, the one real decomposed multi-task template

**Utterance:** `launch my startup`

**Observed:** all four real subtasks (`market_research`, `business_model`, `branding`,
`legal_formation`) tracked and reported individually with real progress states; each result
labeled with its own real, meaningful task name (unaffected by the fix in scenario 2, since that
fix only touches the single-outcome `generic_goal` sentinel, never a decomposed plan's real task
names).

**Status:** Working as designed — no fix needed. Confirmed still true after scenario 2's fix (see
`crates/hyperion-console/tests/console_session.rs::a_decomposed_plans_own_tasks_render_real_generated_content_not_just_a_status_word`).

### 4. Power user, follow-up on a sub-result

**Utterance:** `launch my startup` → `/result market_research`

**Observed:** `/result` correctly retrieves that one task's full real text directly by name, with
a helpful pointer to it already surfaced in the plan's own summary output
(`... (see "/result market_research" for the full text)`).

**Status:** Working as designed — no fix needed.

### 5. Confused user, mistyped command

**Utterance:** `/nonexistent`

**Observed:** silently fell through to the generic-goal path and got echoed by the mock backend
as if it were a real request — no indication the command wasn't recognized.

**Finding:** no feedback loop for an unrecognized `/command`.

**Status:** Fixed — `2602a93`, regression test
`an_unrecognized_slash_command_gets_real_feedback_not_a_silent_agent_dispatch`.

### 6. Lost beginner asking for help, two ways

**Utterance:** `help` (no slash) vs. `/help`

**Observed:** `/help` gave a good, discoverable, plain-language help message. Bare `help` — the
single most natural thing a lost user would type — silently fell through to the generic-goal path
instead.

**Finding:** the real help system was only reachable via the slash form.

**Status:** Fixed — `2602a93`, regression test
`bare_help_with_no_slash_gives_the_same_real_help_text_as_slash_help`.

### 7. Security-sensitive: connecting a paid cloud account

**Utterance:** `connect my openai account` → paste a (fake, test-only) key

**Observed:** clean, well-designed flow — key not echoed or logged, a masked preview shown
(`sk-f...-123, 20 characters`), a clear next step suggested (`try "/backend openai <model>"`).

**Status:** Working as designed — no fix needed.

### 8. Ambiguous reference, then inspecting memory

**Utterance:** `remind me about the api` → `/recall`

**Observed:** `/recall` correctly lists the turn as a low-confidence (30%) recorded utterance, with
no fabricated entity resolution for "the api" (nothing else had been said about it, so there was
nothing concrete to resolve).

**Status:** Working as designed — no fix needed. Not deeply probed beyond this; explaining
confidence percentages to a lay user in plain language is a possible future scenario to add, not
investigated here.

### 9. Multi-turn conversational continuity

**Utterance:** `my name is Alex` → `what is my name`

**Observed (before fix):** zero continuity. Both utterances echoed independently; `/recall` (when
added to the same test) showed both turns existed but nothing connected them.

**Finding (root cause, traced through code, not guessable from output alone):**
`ConsoleSession` minted a brand-new, unique session id on *every single turn* and passed it to
`IntentEngine::handle_utterance` and to `Scope.session_id` in the context-assembly path. Three
already-real, already-tested mechanisms are keyed by that id — `hyperion-intent`'s working-memory
turn buffer, its active-graph reconciliation stack, and `hyperion-context`'s working-set
hysteresis (including this session's own earlier `current_expertise` fix, from before this
scenario sweep) — so none of them ever accumulated more than one turn's history before being
silently discarded and recreated empty on the very next turn, in the one real, booted entry point.

**Status:** Fixed — `335a7e2`. `ConsoleSession` now has one stable `session_id` for its whole
process lifetime, and `run_undecomposed_goal`'s prompt now includes recent conversation via
`prompt_with_recent_history`. Regression test:
`a_followup_utterance_carries_real_conversation_history_into_its_own_prompt`. Note this fix makes
the *mechanism* real (recent turns genuinely reach the prompt); it does not by itself make
`MockBackend` answer "what is my name" correctly — echoing is all a mock backend can ever do; see
open finding below.

## Open findings (named, not fixed)

- **Out-of-the-box default experience is a raw model echo.** `MockBackend::generate` is
  `format!("[mock model {id}] echo: {prompt}")` — deliberately a test fixture, shared across many
  crates' own test suites (changing its output format has a large, out-of-scope blast radius, see
  scenario 2). Anyone cloning this repo and running `cargo run -p hyperion-console` with no extra
  flags gets this as their literal first impression of "the first intent-native operating system."
  A real fix would live at the console layer (e.g. an honest "you're running without a connected
  model" notice on first use of the mock backend), not inside `MockBackend` itself. Not attempted
  in this sweep — scoped out as its own, separate piece of work.
- **Coverage gaps.** No scenario has yet exercised: `hyperion-shell` (needs a real display, not
  drivable headlessly the way `hyperion-console` is), package management, process management,
  multi-user permissions, long-running/multitasking sessions, actual filesystem operations, update
  application, or any failure-injection/recovery scenario. Each is a real "docs/41 Phase 2" gap in
  this file, not something to assume is fine because it wasn't found broken.
