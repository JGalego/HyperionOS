# Changelog

All notable changes to Hyperion are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Hyperion doesn't yet promise Semantic Versioning compatibility guarantees --
version numbers track release sequence, not API stability.

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
