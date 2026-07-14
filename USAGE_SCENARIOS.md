# Hyperion Usage Scenarios — a living acceptance-test log

This document is the durable record of docs/41's Phase 2 ("model real usage") and Phase 3
("compare reality vs. expectations") work: realistic scenarios actually driven against the real,
compiled system, what was observed, and what happened as a result. It is meant to grow the same
way [35 — Testing Strategy](docs/35-testing-strategy.md)'s golden corpus grows — from real
sessions, not only hand-authored cases — except this file is the human-readable log; the durable,
machine-checked form of each finding below lives as a real regression test in the relevant crate
(linked per entry).

**Status: a starting set, not the "dozens" docs/41 asks for.** ~13 scenarios have been run so far,
plus all ten of the per-backend scenario files under [`scenarios/`](scenarios/) (scenario 10
below), against `hyperion-console` only (the one real, natively-buildable, non-GUI entry point
this sandbox can drive with piped stdin or a scenario file — see "How scenarios are run" below).
Scenarios 11 and 13 (AUTONOMY_ROADMAP.md's Resourceful and Self-Sustaining pillars) are the first
two verified via `cargo test` rather than a driven console session — named honestly as such, since
neither has a console-level trigger today; see each entry's own "Open finding."
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

### Checking how the knowledge graph changed

`/graph` (plain text) / `/graph dot` (Graphviz) is a real console meta-command, alongside
`/recall`/`/why`/`/related`/`/result` — but unlike those (each a targeted question about one
thing), it dumps the *whole* recorded graph at once: every real node and edge the current session
can see (`hyperion_knowledge_graph::KnowledgeGraph::dump`), sorted by id. That sort is the whole
point: two dumps of an unchanged graph are byte-for-byte identical (verified live — see
`hyperion-console`'s own `graph_dump::two_consecutive_dumps_with_no_change_in_between_are_identical`
test), so running it once before a scenario and once after, then diffing the two outputs, shows
exactly what changed — new nodes/edges as new lines, nothing as no lines, never ordering noise
misread as a change:

```
rm -rf /tmp/hyperion-scratch
printf '/graph\n' > /tmp/before.txt
HYPERION_CONSOLE_DATA_DIR=/tmp/hyperion-scratch ./target/debug/hyperion-console /tmp/before.txt \
    > before.out

HYPERION_CONSOLE_DATA_DIR=/tmp/hyperion-scratch ./target/debug/hyperion-console \
    scenarios/multi-turn-demo.txt

printf '/graph\n' > /tmp/after.txt
HYPERION_CONSOLE_DATA_DIR=/tmp/hyperion-scratch ./target/debug/hyperion-console /tmp/after.txt \
    > after.out

diff before.out after.out
```

This works against the *same* `HYPERION_CONSOLE_DATA_DIR` across three separate invocations
because the Knowledge Graph is a real, durable WAL, not in-memory-only state — it persists across
process restarts exactly as it does across turns within one process.

`/graph dot` prints the same graph as Graphviz DOT instead — verified live: `dot -Tsvg` renders it
into a real, valid SVG:

```
printf '%s\n' "launch my startup" "/graph dot" > /tmp/graph-demo.txt
./target/debug/hyperion-console /tmp/graph-demo.txt | sed -n '/digraph/,/^}/p' | dot -Tsvg -o graph.svg
```

Deliberately not the default: plain text stays screen-reader-friendly and diffable with plain
`diff`, matching CLAUDE.md's accessibility-first stance — DOT is an opt-in for whoever specifically
wants a picture. Also deliberately shows raw ids and absolute (epoch-second) timestamps rather than
`/why`'s human "recorded 3 minutes ago" phrasing: a relative phrasing would make an unchanged dump
look different depending on *when* you ran it, defeating the whole point of diffing two dumps.

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

### 11. Resourceful — a real installed plugin actually runs (no echo)

**AUTONOMY_ROADMAP.md's Slice 1/1b.** Unlike scenarios 1-10, this one has **no CLI-drivable path
today** — `hyperion-console` never routes an utterance's text into an arbitrary capability's raw
JSON args, so there's no slash command or utterance that reaches `PluginRegistry::invoke_native_binary`
by hand. Real, honest verification here is `cargo test`, run fresh, not assumed from memory:

```
$ cargo test -p hyperion-plugin-framework --test native_binary_execution
running 5 tests
test invoking_an_uninstalled_capability_is_a_real_honest_error ... ok
test installing_a_non_executable_file_as_a_native_binary_is_rejected ... ok
test installing_a_native_binary_with_a_nonexistent_program_is_rejected ... ok
test an_installed_native_binary_actually_runs_and_returns_real_output ... ok
test a_tool_exiting_nonzero_is_a_real_honest_error_not_a_panic ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test -p hyperion-agent-runtime --test plugin_dispatch
running 2 tests
test invoke_falls_back_to_the_stub_echo_when_no_plugin_registry_is_wired ... ok
test invoke_dispatches_an_unrecognized_capability_to_a_real_installed_plugin ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test -p hyperion-api-gateway
running 14 tests
...
test invoke_capability_dispatches_to_a_real_installed_native_binary_plugin ... ok
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Each proves the real thing end to end: a tiny musl-built binary
(`crates/hyperion-plugin-framework/src/bin/uppercase_tool.rs`) is installed with a real
`NativeBinaryDescriptor`, then invoked through the exact real sandboxed path
(`hyperion_trust_boundary::spawn`) via three separate real call sites —
`PluginRegistry::invoke_native_binary` directly, `AgentRuntime::invoke`'s dispatch chain, and
`ApiGateway::dispatch_one`'s own — and its own real stdout comes back, not a canned response.

**Status:** Verified live via `cargo test`, not a fix. **Open finding, named honestly:** there is
no way to demo this from `hyperion-console` today (no utterance or slash command reaches a
plugin's `capability_ref`) — a real gap for whoever picks up making this interactively drivable,
tracked here rather than silently left implicit.

### 12. Social — two real Hyperion processes talk over real MCP and A2A

**AUTONOMY_ROADMAP.md's Slice 2.** Unlike scenario 11, this one *is* fully CLI-drivable — a human
can run every step below by hand. Two real, separately-launched `hyperion-console` processes,
talking over real HTTP, JSON-RPC 2.0 (MCP) and the real A2A spec:

```sh
cargo build -p hyperion-console --bin hyperion-console
printf '/mcp-server 8765\n/standby\n' > /tmp/mcp-demo.txt
HYPERION_CONSOLE_DATA_DIR=/tmp/hyperion-mcp-demo ./target/debug/hyperion-console /tmp/mcp-demo.txt
```
```
> /mcp-server 8765
Real MCP server listening on http://127.0.0.1:8765 -- JSON-RPC 2.0 (initialize, tools/list, tools/call). ...
> /standby
Standing by -- press Enter at this terminal when you're done testing, to stop.
```

From a second terminal, real `curl` calls against the real running server:

```sh
curl -s http://127.0.0.1:8765/ -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
```
```json
{"id":1,"jsonrpc":"2.0","result":{"capabilities":{"tools":{}},"protocolVersion":"2024-11-05","serverInfo":{"name":"hyperion-console","version":"0.1.0"}}}
```

Two `tools/call hyperion.ask` turns in the same connection prove real conversational continuity —
the exact same `prompt_with_recent_history` mechanism scenario 9 established, now reachable over
the wire, not just in-process:

```sh
curl -s http://127.0.0.1:8765/ -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"hyperion.ask","arguments":{"prompt":"my name is Alex"}}}'
curl -s http://127.0.0.1:8765/ -d '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"hyperion.ask","arguments":{"prompt":"what is my name"}}}'
```
```json
{"id":3,"jsonrpc":"2.0","result":{"content":[{"text":"status: done -- [mock model 1] echo: my name is Alex","type":"text"}],"isError":false}}
{"id":4,"jsonrpc":"2.0","result":{"content":[{"text":"status: done -- [mock model 1] echo: Recent conversation, most recent last:\nmy name is Alex\n\nNow respond to: what is my name","type":"text"}],"isError":false}}
```

`/recall` over the same tool confirms both turns landed in the one shared session's real
conversation history (`[1] you asked: \"what is my name\" (30% confident)`, `[2] you asked: \"my
name is Alex\" (30% confident)`).

A2A, the outbound half this time — a real *second* `hyperion-console` process runs `/a2a-call`
against a real running `/a2a-server`, injecting a turn from an entirely separate process into the
server's session:

```sh
printf '/a2a-server 8766\n/standby\n' > /tmp/a2a-demo.txt
HYPERION_CONSOLE_DATA_DIR=/tmp/hyperion-a2a-demo ./target/debug/hyperion-console /tmp/a2a-demo.txt &

printf '/a2a-call 127.0.0.1 8766 my name is Jordan\n' > /tmp/a2a-call-demo.txt
HYPERION_CONSOLE_DATA_DIR=/tmp/hyperion-a2a-caller ./target/debug/hyperion-console /tmp/a2a-call-demo.txt
```
```
> /a2a-call 127.0.0.1 8766 my name is Jordan
status: done -- [mock model 1] echo: my name is Jordan
```

A raw `SendMessage` call against the real spec-defined endpoint, from a third vantage point
(plain `curl`), shows the turn the second process injected is really there in the server's own
memory — not two divergent conversations:

```sh
curl -s http://127.0.0.1:8766/.well-known/agent-card.json
curl -s http://127.0.0.1:8766/ -d '{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":{"messageId":"m1","role":"ROLE_USER","parts":[{"text":"what is my name"}]},"configuration":{"returnImmediately":false}}}'
```
```json
{"capabilities":{"extendedAgentCard":false,"pushNotifications":false,"streaming":false},"id":"hyperion-console","interfaces":[{"type":"json-rpc","url":"/"}],"name":"Hyperion","provider":{"name":"Hyperion","url":"https://github.com/JGalego/HyperionOS"},"skills":[{"description":"A real utterance through Hyperion's real Intent Engine and Agent dispatch.","id":"hyperion.ask","name":"Ask Hyperion"}]}
{"id":1,"jsonrpc":"2.0","result":{"contextId":"task-2","id":"task-2","status":{"message":{"messageId":"task-2-reply","parts":[{"text":"status: done -- [mock model 1] echo: Recent conversation, most recent last:\nmy name is Jordan\n\nNow respond to: what is my name"}],"role":"ROLE_AGENT"},"state":"TASK_STATE_COMPLETED","timestamp":"2026-07-14T18:39:18Z"}}}
```

**Status:** Verified live, exactly as shown above, not a fix. `/standby` is what makes any of this
possible by hand at all — without it, a scenario file ending would tear the whole process (server
included) down before a second terminal could ever reach it. Automated regression coverage for the
same three flows lives in `crates/hyperion-console/tests/mcp_a2a_server.rs`.

### 13. Self-sustaining — a suspended agent auto-resumes, and remembers across a restart

**AUTONOMY_ROADMAP.md's Slice 3/3b.** Like scenario 11, there is no console-level trigger for this
— nothing in `hyperion-console`'s utterance-parsing layer ever constructs the `{"force_fail":
true}` JSON that trips `hyperion-agent-runtime`'s circuit breaker on purpose; that's a raw
`AgentRuntime::invoke` argument only reachable from Rust code today. Real, honest verification is
`cargo test`, run fresh:

```
$ cargo test -p hyperion-agent-runtime --test adaptive_backoff
running 4 tests
test an_immediate_retry_after_suspension_gets_an_honest_still_recovering_message ... ok
test a_real_success_streak_after_resume_decays_times_suspended_back_down ... ok
test after_the_real_backoff_window_elapses_the_instance_auto_resumes_and_actually_runs ... ok
test a_second_suspensions_backoff_is_measurably_longer_than_the_first ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.22s

$ cargo test -p hyperion-agent-runtime --test cross_session_learning
running 2 tests
test a_different_specialization_has_no_remembered_history ... ok
test a_specializations_suspension_history_survives_a_real_restart ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Together these prove, with real wall-clock waits (no clock-injection seam in this crate) and a
real on-disk Knowledge Graph opened twice against the same path (simulating a genuine process
restart): three consecutive real failures suspend an instance; an immediate retry gets an honest
"still recovering, try again" message instead of a bare technical error; the instance auto-resumes
and actually runs once its real adaptive backoff window elapses; a *second* suspension's backoff
is measurably longer than the first (the system gets more cautious about a repeat offender); a
real streak of consecutive successes after a resume decays that caution back down (it "comes out
stronger," not permanently scarred); and none of this resets to a blank slate across a real
restart — a specialization's own suspension history survives, while an unrelated specialization's
fresh instance still starts at zero.

**Status:** Verified live via `cargo test`, not a fix. **Open finding, named honestly:** same gap
as scenario 11 — no interactive way exists yet to force a real capability failure from the
console, so this pillar can only be demonstrated today by someone willing to read Rust test code.

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
