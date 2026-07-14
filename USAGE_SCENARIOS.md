# Hyperion Usage Scenarios — a living acceptance-test log

This document is the durable record of docs/41's Phase 2 ("model real usage") and Phase 3
("compare reality vs. expectations") work: realistic scenarios actually driven against the real,
compiled system, what was observed, and what happened as a result. It is meant to grow the same
way [35 — Testing Strategy](docs/35-testing-strategy.md)'s golden corpus grows — from real
sessions, not only hand-authored cases — except this file is the human-readable log; the durable,
machine-checked form of each finding below lives as a real regression test in the relevant crate
(linked per entry).

**Status: a starting set, not the "dozens" docs/41 asks for.** ~10 scenarios have been run so far,
plus all ten of the per-backend scenario files under [`scenarios/`](scenarios/) (scenario 10
below), against `hyperion-console` only (the one real, natively-buildable, non-GUI entry point
this sandbox can drive with piped stdin or a scenario file — see "How scenarios are run" below).
`hyperion-shell` (a real
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

### Running a scenario from a file

For anything beyond a one-off utterance, pass a scenario *file* as the binary's own first
argument instead of building a `printf` command by hand:

```
cargo build -p hyperion-console --bin hyperion-console
HYPERION_CONSOLE_DATA_DIR=<a real, disposable directory> \
    ./target/debug/hyperion-console scenarios/multi-turn-demo.txt
```

One real utterance per line. A blank line or a `#`-prefixed comment is skipped, not sent as a
real (nonsensical) utterance — so a scenario file can document itself and stay readable. `$NAME`
is expanded against the real process environment before the line is sent, so a checked-in
scenario file can reference `$OPENAI_API_KEY` by name (never a real literal secret) and still
drive a real connection once `source .env && export $(grep -v '^#' .env | xargs)` — or just
`set -a && source .env && set +a` — has actually set it. A pasted-key line is still redacted in
the printed transcript exactly as a real terminal never echoes it. Each utterance is echoed as
`"> {utterance}"` right before its own real response, so the output reads as a self-contained
transcript; the binary exits as soon as the file ends rather than falling through to interactive
stdin. See [`scenarios/multi-turn-demo.txt`](scenarios/multi-turn-demo.txt) for a real, runnable
example of the scenario in "A more complex, multi-turn, multi-request-type scenario" below.

### Running a scenario against a real backend

Every scenario above ran on `MockBackend`. Each backend gets its secret differently — traced
through `hyperion-console::session.rs`, not guessed — and each now has its own real, runnable
scenario file under [`scenarios/`](scenarios/) instead of a hand-built `printf` command:

| Backend | Scenario file | Build feature | Needs |
| --- | --- | --- | --- |
| Mock (default) | [`backend-mock.txt`](scenarios/backend-mock.txt) | none | nothing |
| Candle (local inference) | [`backend-candle.txt`](scenarios/backend-candle.txt) | `candle` | network, first run only (HF Hub download) |
| Ollama | [`backend-local-ollama.txt`](scenarios/backend-local-ollama.txt) | `openai-compat` | a real `ollama serve` with the model pulled |
| vLLM | [`backend-local-vllm.txt`](scenarios/backend-local-vllm.txt) | `openai-compat` | a real vLLM OpenAI-compatible server |
| LiteLLM | [`backend-local-litellm.txt`](scenarios/backend-local-litellm.txt) | `openai-compat` | a real LiteLLM proxy, `HYPERION_LITELLM_API_KEY` if it needs one |
| Custom OpenAI-compatible | [`backend-local-custom.txt`](scenarios/backend-local-custom.txt) | `openai-compat` | your own server's base_url + model, edited into the file |
| OpenAI | [`backend-cloud-openai.txt`](scenarios/backend-cloud-openai.txt) | `openai-compat` | a real `OPENAI_API_KEY` in `.env` |
| Anthropic | [`backend-cloud-anthropic.txt`](scenarios/backend-cloud-anthropic.txt) | `anthropic` | a real `ANTHROPIC_API_KEY` in `.env` |
| Gemini | [`backend-cloud-gemini.txt`](scenarios/backend-cloud-gemini.txt) | `gemini` | a real `GEMINI_API_KEY` in `.env` |
| Groq | [`backend-cloud-groq.txt`](scenarios/backend-cloud-groq.txt) | `openai-compat` | a real `GROQ_API_KEY` in `.env` |

Each file's own header comment names its exact build/run command. The general shape:

```
cargo build -p hyperion-console --bin hyperion-console --features <feature from the table>
set -a && source .env && set +a   # only for local engines with a key, or any cloud provider
rm -rf /tmp/hyperion-scratch
HYPERION_CONSOLE_DATA_DIR=/tmp/hyperion-scratch \
    ./target/debug/hyperion-console scenarios/<file from the table>
```

**Build `candle` on its own, in its own binary — never combined with the other features in one
build.** This is not just tidiness: `ConsoleSession::build_ai_runtime` checks `cfg!(feature =
"candle")` — a compile-time flag read at runtime — so *any* binary built with `--features candle`
tries to load a real Candle model eagerly at startup, on every launch, regardless of which
scenario file you actually pass it or whether it ever says `/backend candle`. Verified live: a
binary built with `--features openai-compat,anthropic,gemini,candle` together took several
minutes just to *start up* (a real, first-time Hugging Face Hub download blocking
`ConsoleSession::open`) before any of the local-engine or cloud scenarios below could even begin —
purely from `candle` being compiled in, with no `/backend candle` anywhere in those files. Keep a
`candle`-only binary and an `openai-compat,anthropic,gemini`-only binary as two separate builds.

- **Candle.** No secret at all — it downloads a small public model from Hugging Face Hub with no
  auth needed, the *first* time it runs (verified live: tens of seconds to a few minutes,
  depending on real network conditions — not something to run inside a tight timeout).
- **Local engines (ollama/vllm/litellm/custom).** Each reads its own dedicated environment
  variable at `/backend` switch time (`EngineKind::api_key_env_var`), all optional (Ollama/vLLM
  usually don't need one; a self-hosted LiteLLM proxy often does). **Corrected from an earlier
  draft of this doc, after testing against both a genuinely dead port and a real running server:**
  `OpenAiCompatBackend::connect` (`crates/hyperion-ai-runtime/src/openai_compat_backend.rs`) makes
  a real, eager `GET {base_url}/models` call at switch time, and only ever tolerates *one* specific
  failure softly — the server responds, but your model name isn't in its list (a real, warn-and-
  continue case: some servers format ids differently, and a genuinely wrong name still surfaces
  honestly on the first real request instead). Every other failure — nothing listening on that
  port at all, a non-2xx response, anything reqwest itself can't complete — is a real, immediate,
  hard "I couldn't switch" error, not a lenient one. Verified live against a real Ollama server
  actually running in this sandbox (`llama3.2:1b` real model pulled): a plain `ollama serve` with
  no such tag hit the soft path (warned, then a real 404 on the next request); vLLM/LiteLLM/custom
  with nothing listening on their ports all hit the hard path immediately, correctly leaving the
  session on `MockBackend`, not half-switched.
- **Cloud providers (openai/anthropic/gemini/groq).** Deliberately *not* read from the environment
  by the console itself — the only real path in is the interactive `connect my <provider> account`
  utterance, which stores the key encrypted at rest. Each provider needs its own build feature to
  actually connect (`try_connect_openai`/`try_connect_anthropic`/`try_connect_gemini`/
  `try_connect_groq` each fail with an honest, named error otherwise) — OpenAI's and Groq's cloud
  APIs both reuse `openai-compat` (Groq's own API is wire-compatible with OpenAI's, same as a
  local engine's, but it's still a real, paid, third-party cloud API, so it's gated as a
  `CloudProvider`, not an `EngineKind`); Anthropic and Gemini each need their own dedicated
  feature. **`/backend <provider> <model>` does a real, eager connectivity check right then** —
  verified live against all four real providers' real APIs, with a deliberately fake key each
  time: the switch itself made a real HTTPS call and failed outright (OpenAI, Anthropic, and Groq
  each with a real `401 Unauthorized`; Gemini with a real `400 Bad Request` instead — a genuine,
  provider-specific difference, not a bug in this codebase), and the session correctly stayed on
  whichever backend was active before rather than half-switching. You need a genuinely valid key
  for the switch to succeed at all. **Corrected from an earlier draft of this doc:** connecting
  and switching in the *same* session grants that one running session immediate use — verified
  live (with a fixture server standing in for the real API) via `hyperion-console`'s own
  cloud-consent-lifecycle test (`crates/hyperion-console/tests/console_session.rs`). No `yes`/`no`
  consent line is needed in any of the four scenario files above; a real "yes/no" consent prompt
  only fires on a *fresh process* reusing an already-stored key without reconnecting first (not
  covered by a scenario file yet — would need a second file that assumes the first already ran).

### A more complex, multi-turn, multi-request-type scenario

Combining several request shapes and a mid-session backend switch in one real session — this is
the shape to follow for new complex scenarios, not just single-utterance ones. As a real, checked-
in scenario file — see "Running a scenario from a file" above:

```
rm -rf /tmp/hyperion-scratch
HYPERION_CONSOLE_DATA_DIR=/tmp/hyperion-scratch \
    ./target/debug/hyperion-console scenarios/multi-turn-demo.txt
```

Or the same thing without a file, one utterance per shell argument (`printf '%s\n' "utterance
one" "utterance two" ...`, not a single multi-line quoted string — a `\` at the end of a line
*inside* a single-quoted string is not a shell line-continuation, it's a literal
backslash-then-newline in the piped text, which silently corrupts the input):

```
rm -rf /tmp/hyperion-scratch
HYPERION_CONSOLE_DATA_DIR=/tmp/hyperion-scratch printf '%s\n' \
    "my name is Alex" \
    "what is my name" \
    "launch my startup" \
    "/result market_research" \
    "/backend candle" \
    "what programming language should I learn first" \
    | ./target/debug/hyperion-console
```

This exercises, in one session: plain conversation + continuity (scenario 9), the one real
decomposed multi-task template (scenario 3), a sub-result lookup (scenario 4), a backend-switch
attempt mid-session, and a plain follow-up. **`/backend candle` is a Cargo feature, not a runtime
option** — it can only ever succeed if *this binary* was compiled with `--features candle` in the
first place; on the default build it correctly, honestly refuses ("I couldn't switch: this build
wasn't compiled with real inference support") and the session keeps working on whichever backend
was already active. Run this scenario twice to see both real behaviors: once against the default
build (confirms the honest refusal) and once against a binary built with `--features candle` from
the start (confirms the switch succeeds and the final answer is real generated text, not an echo).

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

### 10. Running each backend for real, against its own scenario file

**Utterance:** each of `scenarios/backend-{mock,candle,local-ollama,local-vllm,local-litellm,
local-custom,cloud-openai,cloud-anthropic,cloud-gemini,cloud-groq}.txt`, run against a real build
with the matching feature.

**Observed:** all ten ran and produced the behavior their own header comments claim — including
two things an earlier draft of this file's "Running a scenario against a real backend" section
got wrong, both corrected in place rather than left standing:

1. A build with `--features candle` combined with the other features loads a real Candle model
   *eagerly at every startup* — `ConsoleSession::build_ai_runtime` checks `cfg!(feature =
   "candle")`, a compile-time flag, unconditionally — so it blocked `ConsoleSession::open` for
   several minutes (a real, first-time Hugging Face Hub download) before any of the other nine
   scenario files' own utterances could even begin, with no `/backend candle` in any of them. Not
   a bug — a real, deliberate, already-documented design choice for a boot image that wants Candle
   as the default backend with no separate switch step — but a genuine surprise for a dev binary
   built with every feature bundled together for convenience.
2. A local-engine backend switch is lenient about exactly one failure (the server responds, but
   your model name isn't in its list) and hard about everything else (nothing listening on that
   port at all, a non-2xx response) — an earlier draft claimed the switch was lenient outright,
   based on a run where a real local server (this sandbox's own already-running Ollama instance,
   unnoticed at the time) happened to answer.

**Status:** Verified live, not a fix — this file's own "Running a scenario against a real
backend" section carries the corrected, verified claims and the ten scenario files themselves.

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
