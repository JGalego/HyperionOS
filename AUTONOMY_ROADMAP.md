# Hyperion Autonomy Roadmap

CLAUDE.md's "Autonomy" section states the commitment in full — resourceful, social,
self-sustaining. This document is the living record of what's actually real today versus what's
deliberately deferred, in this project's own "deliberately deferred, and why" convention (see
every crate's own doc comment for precedent). Nothing below is marked real until it's built,
tested, and gated (`cargo build/test/fmt/clippy`) — the same standard `PRODUCTION_BOOT_PROMPT.md`
and `USAGE_SCENARIOS.md` already hold themselves to.

## Resourceful — use existing tools, create new ones

**Real today:**

- `hyperion-plugin-framework::PluginRegistry` already does real Ed25519-signed
  install/uninstall/query of capability implementations (PRODUCTION_BOOT_PROMPT.md M9).
- `hyperion-trust-boundary::spawn` already does real Linux sandboxing (user namespaces, Landlock,
  seccomp-bpf) of a forked process (M2).
- **Slice 1, landed.** Those two connect for real: `ImplementationDescriptor`/`CapabilityManifest`
  carry a real `NativeBinaryDescriptor` for `ImplementationKind::NativeBinary`, validated (program
  must really exist and really be executable) at install time;
  `PluginRegistry::invoke_native_binary` runs it inside a real `hyperion_trust_boundary::spawn`
  sandbox (real temp-dir I/O, a real bounded timeout, a real non-blocking `try_wait` poll loop —
  fixed live after an earlier version hung forever, since `is_alive()` alone can't distinguish "still
  running" from "exited but unreaped"); `AgentRuntime::invoke` dispatches an unrecognized
  `capability_ref` to it when a wired `PluginRegistry` has a matching installed implementation,
  instead of falling through to `stubs::dispatch`'s echo. Proven end to end: a real, statically
  linked (musl) tool, installed and invoked through the real console/agent-runtime path, produces
  its own real output.

**Also being built this pass** (pulled forward from an earlier, more conservative draft of this
roadmap, per direct instruction — see this file's own git history for what "deferred" used to
mean here):

- **Tool *creation*.** An agent authoring a brand-new plugin end to end: drafts a small script and
  manifest, runs it through `hyperion-sdk`'s existing harness/validation, installs it through
  `PluginRegistry::install` — using Slice 1's execution path, now real.
- **`hyperion-api-gateway`'s parallel gap.** It holds its own `Arc<PluginRegistry>` with the exact
  same "data only, no execution" problem independently documented in its own code; wired to the
  same execution path Slice 1 builds for `hyperion-agent-runtime`.

**Deliberately still deferred:**

- **The other seven `Contribution` variants** (`Agent`, `HardwareSupport`, `KnowledgeProvider`,
  `UiComponent`, `ExecutionEngine`, `AutomationWorkflow`, `MemoryProvider`) — none has an owning
  subsystem with a real registration point yet; see `hyperion-plugin-framework`'s own doc comment.

## Social — connect with other Hyperion instances

**Real today:**

- *(pending this session's Slice 2)* `hyperion-console --mcp` speaks real MCP over stdio,
  exposing a handful of real capabilities (ask/recall/graph) as tools — the *server* side of "can
  be talked to via a known protocol."

**Also being built this pass:**

- **An MCP client.** Hyperion calling *out* to a real, already-known MCP endpoint — including
  another Hyperion instance's own `--mcp` server. This is not the same as real discovery: the
  endpoint is given, not found.

**Deliberately still deferred:**

- **Real cross-instance discovery, identity, and trust.** Every existing multi-device concept in
  this workspace (`hyperion-federation`, `hyperion-device`) models *one user's own devices* inside
  one process — there is no concept anywhere of a *different* user/instance as a peer yet. This is
  a real, separate identity model, not a small extension of what exists.
- **A2A, gossip, or any custom/invented protocol.** Worth exactly when a real, concrete need
  outgrows what MCP already covers — not before.
- **Real network transport for federation** (`hyperion-federation`'s own deferred list: heartbeat
  timing, ambient anti-entropy, `SyncEnvelope`-wrapped encrypted payloads) — orthogonal to, and a
  prerequisite for, real multi-*device* (not just multi-*instance*) social behavior.

## Self-Sustaining — degrade safely, recover, come out stronger

**Real today:**

- `hyperion-agent-runtime`'s circuit breaker already suspends an instance after
  `CIRCUIT_BREAKER_THRESHOLD` consecutive failures (M8-era).
- `hyperion-recovery`'s undo/redo/crash-recovery journal already exists (real, but purely
  reactive — restores last-known-good, no learning).
- `hyperion-supervisor`'s exponential backoff + give-up policy already exists for OS *processes*.
- *(pending this session's Slice 3)* A suspended `AgentInstance` now has a real way back: it
  auto-resumes after an adaptive backoff window, and a real repeat-offense history makes that
  backoff longer next time, decaying back down after a real streak of successes — the actual
  "recovers, and comes out stronger" mechanic, not just "back to baseline."

**Also being built this pass:**

- **Cross-session learning.** Feeding suspend/recover history into `hyperion-memory`'s Procedural
  tier as a durable, cross-session "lessons learned" store (e.g. "capability X failed repeatedly,
  start more cautious next time"), so the adaptive backoff above survives a restart instead of
  resetting.

**Deliberately still deferred:**

- **`hyperion-recovery` learning from what it rolls back.** Still purely reactive; no mechanism
  connects a rollback's cause to a future decision.
- **A model-router-style "demote, never remove" signal for agent instances generally** — the
  closest existing precedent (`hyperion-model-router`'s circuit breaker, fed by `report_outcome`)
  is scoped to model selection only.
