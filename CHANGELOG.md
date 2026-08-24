# Changelog

All notable changes to Hyperion are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Hyperion doesn't yet promise Semantic Versioning compatibility guarantees --
version numbers track release sequence, not API stability.

## [Unreleased]

### Fixed

**Enforcement and durability**
- `Contribution::ExecutionEngine` could never have worked for a real
  interpreter. `resolve_via_engine` hands a launcher a script path as an
  argument, but `apply_landlock` grants a sandboxed process exactly two things
  -- its own program path, and `fs_scope` (a per-invocation temp directory) --
  and a script is neither, so the launcher was told to run a file it could not
  open. The existing end-to-end test missed it because its companion binary only
  echoes the script path and never reads it. `NativeBinaryDescriptor` now
  carries a `script`, validated at install time the same honest way `program`
  already is, granted through a new `SpawnGrant::read_only_paths` as a Landlock
  `ReadFile` rule on exactly that one file. `program` and `script` are not quite
  symmetric, and the asymmetry is the load-bearing part: `program` must be
  executable, so it can never name an ordinary sensitive file, while a `script`
  is only ever read. Validation therefore uses `symlink_metadata`, not
  `metadata` -- a link would otherwise pass `is_file()` and have Landlock grant
  `ReadFile` on whatever it really pointed at.
- A Trust Boundary is now refused rather than reported as sandboxed when the
  kernel applied no Landlock restrictions at all. `CompatLevel::BestEffort`
  returns `Ok` from `restrict_self()` on a kernel without Landlock, and nothing
  checked the resulting `RulesetStatus` -- so `spawn` reported success while the
  child ran completely unconfined.
- FUSE writes are committed on `flush` rather than only on `release`, so
  close-to-open consistency holds. `release` is asynchronous and `close(2)` never
  waited for it, meaning a file read straight after being written could observe
  the empty object `create` made.
- Local `/mcp-server`, `/a2a-server`, and `/mesh-dashboard` no longer send
  `Access-Control-Allow-Origin: *`, and refuse cross-site `Origin` and
  non-loopback `Host` headers -- any web page could previously drive a real
  agent turn on these unauthenticated endpoints and read the result. Request
  reads are now bounded by a timeout, a header cap, and a body cap.
- `robots.txt`: a malformed bare `User-agent:` line no longer matches every
  crawler (which silently blocked whole sites), and an equal-length `Allow`
  now beats a `Disallow` per RFC 9309 §2.2.2 instead of resolving by file order.
- Stopping an audit-ledger verification schedule or a federation lease heartbeat
  now returns promptly instead of blocking for up to a full interval -- including
  in `Drop`, where a production-scale interval meant a hang nobody would suspect.

**The boot path, once CI could see it**
- `console-drive.py` never typed anything: it waited for a greeting string the
  console does not print, and counted prompts in a way ANSI colour makes
  impossible. `boot-benchmark.py` had both bugs too. Both now share
  `console_stream.py`, which matches on the prompt itself, and the driver has its
  own test that replays a real console stream in seconds rather than only behind
  an hour-long image build.

**Tests and documentation**
- Five timing-dependent tests no longer assert on wall-clock budgets or fixed
  sleeps; they poll for the outcome, or check that operations' time spans
  genuinely overlap. One of them had turned CI red on macOS.
- 58 dead intra-doc links repaired across the workspace.

### Added

**Apps Hyperion builds for you** (docs/998-roadmap.md's new App Builder section,
M1 -- the ladder's first two rungs, T0 "answer" and T1 "tool")
- New `hyperion-app` crate. A goal plus a script becomes a real, signed,
  installed Capability, dispatched through the same real Landlock/seccomp
  sandbox a hand-installed native binary already runs in -- `hyperion_sdk::
  publish` -> `PluginRegistry::install` -> `invoke_native_binary`, no second
  execution mechanism.
- A real typed input contract. An app's declared inputs -- name, type, whether
  required, and a plain-language description -- are encoded *inside* the
  manifest that gets signed, so they cannot drift from the implementation they
  describe and `/apps` has no side file to read. A missing, unknown,
  wrong-typed, or directory-escaping argument is refused in a sentence a person
  can act on, before anything is spawned. `SemanticContract.inputs` was a bare
  `Vec<String>`, which could not be prompted for, validated, or filled from
  context.
- Console meta-commands: `/build`, `/apps`, `/app`, `/run`, `/app-remove`, and
  `/app-engine`. Deliberately the expert surface, not the intended way to reach
  an app -- the primary path stays intent, per docs/01.
- Driving `/build` end to end against the real OpenAI API found three defects
  nothing local would have: the prompt never named the registered engine (a
  model asked for `sh` wrote Python), never said the input and output files
  arrive as `argv[1]`/`argv[2]` (so scripts opened `input.json` by relative
  name), and `/run` was denied outright, because `broker::resolve_grant` refuses
  any capability not on the instance's own manifest and the console's assistant
  manifest is fixed at session open. Each app run now gets a dedicated instance
  whose entire authority is the one app being run. `/run` also understands
  quoted values -- `text="hello there world"` is the most ordinary thing anyone
  would type, and it failed.
- A real goal utterance now gets a line naming the installed app it looks like
  it was about. It suggests rather than runs: an app is generated code and the
  match is word overlap, so auto-running a wrong guess would execute a program
  nobody asked for. Appended to an answer that already happened, never
  substituted for it; a tie between two equally-matching apps says nothing.
- Recorded, because only running it could establish it: a shell is a poor
  execution engine here. Landlock grants `Execute` on the launcher alone, so a
  script cannot run another program -- which is what a shell is for. Asked for
  `sh`, models reached for `jq` and `wc` even when forbidden; asked for
  `python3`, the same models wrote correct self-contained scripts first time.
- `PluginRegistry::capability_entries` and `execution_engine_ids`. The registry
  could answer "is *this* installed" but never "what is installed", so any
  caller wanting to list things had to keep a parallel record that an uninstall
  elsewhere would silently invalidate.

**CI**
- The boot-image jobs now actually run. Their `if:` condition
  (`contains(head_commit.modified, 'boot/')`) could never be true, so no commit
  had ever boot-tested an image.
- New jobs: website lint/typecheck/build (previously validated only at deploy
  time, after merge), `cargo doc` with broken links denied, `cargo audit` on
  dependency changes and weekly, and a job that compiles and tests the optional
  features -- `real-http`, the cloud backends, mDNS -- which no job had ever
  built. Turning them on found a failing test that had never executed anywhere.
- `--locked` on every cargo invocation, least-privilege `permissions`, and
  `concurrency` groups.
- A pull-request template carrying CLAUDE.md's own six PR questions.

**Distribution and contribution**
- `hyperion-console` now ships as a release asset for Linux x86_64 and both macOS
  architectures -- smoke-tested before signing, and signed with the same Ed25519
  release identity the disk images use. Trying Hyperion no longer requires
  flashing a USB stick or building from source.
- `claims.toml` pairs 34 load-bearing guarantees with the exact test that fails
  if each stops being true; `scripts/check-claims.py` enforces the mapping in CI,
  so renaming a test names the guarantee that just lost its evidence.
- `CONTRIBUTING.md` and issue templates, including one for the case this codebase
  is prone to: the code works as written and the documentation overreaches.

### Security
- `webbrowser` advanced to 1.2.4 (RUSTSEC-2026-0257). The two remaining
  `quick-xml` advisories are accepted in `.cargo/audit.toml` with their reasoning
  and their actual way out recorded.

## [0.3.0] -- 2026-07-17

### Added

**Federation & peer trust**
- Real X25519 key-exchange between genuinely independent devices, a real
  lease-renewal heartbeat closing federation's timing gap, and a real TCP
  socket transport (encrypted + signed) carrying ledger publications between
  independent `FederationHub` instances.
- Trust-on-first-use peer identity checks for `/a2a-call` and `/mcp-call`,
  real MCP `resources/list`/`resources/read` support, a real A2A task store
  (`GetTask`/`ListTasks`), and a real MCP stdio transport (`--mcp-stdio`).
- Real mDNS/DNS-SD advertise+discover for `/mcp-server` and `/a2a-server`.

**Plugin framework: a complete contribution surface**
- Real registration points for every remaining `Contribution` kind --
  `Agent`, `HardwareSupport`, `KnowledgeProvider`, `UiComponent`,
  `AutomationWorkflow`, `MemoryProvider`, and `ExecutionEngine`.
- Consent-diffing `plugin_update`, real `version_variant()` minting, a real
  per-implementation privacy tier, and a real multi-publisher trust store.

**Security, privacy & access control hardening**
- Owner-based ACL enforcement on the Knowledge Graph's single-object
  accessors and its `link()` update path, plus a capability check and
  Trust-Boundary gate on `hyperion-explainability`'s `explain.query` and on
  `expire_lapsed_soft_deletes`.
- Real soft-delete grace-period expiry, `CryptoShred` erasure wired to the
  real `delete_node`, and lapsed soft-deletes now genuinely shredded.
- Real Ed25519 signing and replay resistance for `hyperion-capability`'s
  `WireToken`, and a real seccomp/Landlock IPC-rights dimension for the
  rendezvous socket.

**Explainability & observability**
- Rolling Brier-score calibration tracking, `ConfidenceMethod::SelfConsistency`,
  and a real background scheduled chain-verification job.
- A real globally-unique cross-device span identity and
  `get_rationale`-by-`invocation_id`.
- New signals distinguishing judgment/taste/empathy from risk, and "was this
  meaningful" from speed; an opt-in "think" checkpoint before intent
  decomposition; a real teaching-mode capability (`/teach <topic>`); and a
  real skill delegation-count signal for the Protect-the-Human backlog.

**Knowledge Graph, memory & recovery**
- Real node deletion (tombstone), an inferred-edge pruning sweep, inferred-edge
  decay for co-occurs-with edges, and nested JSON-LD relationships now
  extracted as real edges.
- `hyperion-recovery` now learns from rollback causes and `hyperion-update`
  refuses to repeat one it already learned from; real un-creation in
  recovery's `undo`; pinning-aware recovery-point compaction; and a real
  anti-rollback monotonic counter for system image updates.
- Real AI-backed Working->Episodic memory distillation, model-estimated
  salience, semantic summarization wired into `hyperion-context`, and
  retention/rollup compaction for metrics, logs, and storage versions.

**Scheduling, scale & routing**
- Real model-tier degradation, `Implementation.resourceProfile` threaded into
  scheduler admission, a real `Substitution` -> resource-footprint mapping,
  and real object-affinity plan partitioning.
- Percentage-based canary traffic splitting and a `cloud_consent` check on
  the Model Router bridge; a sigma-based statistical-significance regression
  gate in `hyperion-release-gate`; and a real BLAKE3 `package_hash` content
  fingerprint in `hyperion-sdk`.

**Console & website**
- Real many-instance capability delegation with a live dashboard, plus a
  matching many-instance mesh delegation demo on the website's live console
  section.
- Tasteful color and status symbols throughout `hyperion-console`, a dense
  physically-tinted starfield intro, re-recorded terminal demos, and the
  website deployed live at try-hyperion.org.

### Fixed

- Retried transient connect/write failures in
  `publish_ledger_over_socket` -- macOS CI intermittently saw
  `ConnectionReset`/`BrokenPipe` connecting to a just-bound listener whose
  accept-loop thread hadn't polled yet.
- Fixed real cross-peer conversation bleed in A2A `SendMessage`.
- Fixed intro-time main-thread contention that stalled in-page anchor
  scrolls on the website.

## [0.2.0] -- 2026-07-15

### Added

**Autonomy: Resourceful, Social, Self-Sustaining**
- Real sandboxed execution of installed capability plugins, wired end-to-end
  from the API gateway through to the plugin runtime.
- `hyperion-sdk` now publishes real, runnable native-binary tools instead of
  stub definitions.
- `hyperion-console` speaks real MCP and A2A (agent-to-agent) protocols, both
  as a server and a client, so Hyperion instances can discover and collaborate
  with peers.
- Adaptive backoff auto-resume for suspended agent instances, plus
  cross-session learning: Hyperion now remembers past suspend/recover history
  and uses it to make better resume decisions next time.

**Multi-backend AI runtime**
- Real local-engine inference backends: Ollama, vLLM, and LiteLLM.
- Real cloud provider backends -- OpenAI, Anthropic, Gemini, and Groq -- behind
  a real user consent gate.
- A runtime backend switch in the console, so users can move between local and
  cloud models without restarting.
- Real Candle-based local inference working end-to-end inside the boot image,
  with zero network dependency.

**Console experience**
- Startup banner and a hardened connect-account flow.
- Stable per-session identity and real conversation history for
  `ConsoleSession`, backed by its own data directory.
- New `/graph`, `/recall`, `/why`, and `/related` commands to explore and
  explain the Knowledge Graph directly from the console.
- Real, actionable feedback for bare `help` and unrecognized slash commands.
- Support for running a saved scenario straight from a file
  (`hyperion-console <SCENARIO>`), plus a set of real, runnable per-backend
  scenario files.

**"Launch my startup" reference scenario**
- Produces real generated content (not placeholder status text), with
  live, real-time feedback and a way to steer the results mid-run.
- `hyperion-shell`: a real visual renderer for the compiled Workspace.

### Changed

- Hardened several previously-deferred subsystems with real implementations:
  Ed25519 signing for context envelopes and device-registration manifests,
  periodic signed Merkle anchors for the observability audit ledger, a real
  crash-loop give-up/alerting policy in the supervisor, real historical-version
  reads and per-object ACL enforcement in the Knowledge Graph, real `redo()`
  in the recovery subsystem, and a working-set-derived signal for
  `ContextEngine.currentExpertise`.
- `AlternateImplementation` substitutions are now confirmed against a real
  plugin registry instead of an assumed one.
- Consolidated all root-level documentation into `docs/`, keeping only
  `README.md` and `CLAUDE.md` at the repository root.
- Relicensed the project under MIT and published the official Hyperion
  website, with a refreshed, animated README banner.

### Fixed

- Fixed stale response text carrying over across multiple console turns.
- Dropped the meaningless internal `generic_goal:` label leaking into single
  requests.

## [0.1.0] -- 2026-07-12

Initial automated, signed release: builds and boot-tests both reference
platforms (x86_64, aarch64) and publishes Ed25519-signed images as GitHub
Release assets.
