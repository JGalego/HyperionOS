# Changelog

All notable changes to Hyperion are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Hyperion doesn't yet promise Semantic Versioning compatibility guarantees --
version numbers track release sequence, not API stability.

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
