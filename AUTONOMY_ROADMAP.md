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

- **Tool *creation*, landed (in the safe, honest sense this workspace can support today).**
  `hyperion-sdk::Implementation` carries a real `native_binary: Option<NativeBinaryDescriptor>`,
  threaded through `prepare_submission` → `publish` → `PluginRegistry::install`: naming an
  existing, real, already-vetted program as a `Runtime::NativeBinary` submission now installs it
  as a genuinely *runnable* capability, invocable through Slice 1's real execution path the moment
  `publish` returns — proven end to end. **Deliberately not built**: an agent synthesizing brand-new
  code from scratch and directly executing it. Real code review/static analysis of freshly
  generated code before ever running it is separate, substantial work this session didn't
  attempt — naming it here rather than quietly skipping it or faking it with an unreviewed
  auto-exec path, which would be a real security regression, not a feature.
- **`hyperion-api-gateway`'s parallel gap, landed.** Its own `ApiGateway::dispatch_one` now checks
  `self.registry` for a runnable `NativeBinary` implementation before falling back to
  `hyperion_agent_runtime::dispatch_stub_capability`, the exact same real execution path Slice 1
  built — proven end to end the same way, through `invoke_capability`.

**Deliberately still deferred:**

- **The other seven `Contribution` variants** (`Agent`, `HardwareSupport`, `KnowledgeProvider`,
  `UiComponent`, `ExecutionEngine`, `AutomationWorkflow`, `MemoryProvider`) — none has an owning
  subsystem with a real registration point yet; see `hyperion-plugin-framework`'s own doc comment.

## Social — connect with other Hyperion instances

**Real today:**

- **`/mcp-server [port]`** — a real MCP (Model Context Protocol) server, started as a real
  background thread from an ordinary console meta-command, over real HTTP (JSON-RPC 2.0:
  `initialize`, `tools/list`, `tools/call`) rather than stdio — stdio stays free for the rest of
  the session, unlike a `--mcp`-flag design that would need to own it exclusively. Exposes
  `hyperion.ask`/`hyperion.recall`/`hyperion.graph` as real tools, each a real turn through the
  exact same `ConsoleSession::handle_utterance` path everything else in this crate uses — no new
  bypass of the capability/consent model.
- **`/a2a-server [port]`** — a real A2A (Agent2Agent) server, same shape: a real Agent Card at the
  real spec-defined `/.well-known/agent-card.json`, and the real `SendMessage` JSON-RPC method
  (the spec's own minimal "send a message, get a reply" flow), backed by the same live session.
- **`/mcp-call <host> <port> <tool> <json args>`** and **`/a2a-call <host> <port> <message
  text>`** — the real outbound half: Hyperion calling *out* to a real, already-known MCP/A2A
  endpoint, including another Hyperion instance's own server. Verified live: two real
  `hyperion-console` processes, one running `/a2a-server`, the other running `/a2a-call` against
  it, genuinely exchanging a real reply pulled from the *first* process's own conversation history.
  Not discovery — the endpoint is named, not found (see deferred, below).
- **`/standby`** — blocks on a real read of this process's own stdin until the user provides real
  input, then exits. Exists specifically so a scenario that starts a background server doesn't
  have the whole process (server included) exit the instant the scenario file ends — the real
  mechanism for "keep this alive long enough to test the server from another terminal, on my own
  schedule."
- Both servers share one real `Arc<Mutex<ConsoleSession>>` with the console's own interactive/
  scenario-file loop — a real MCP/A2A tool call and a real typed utterance affect (and can observe)
  the very same conversation, not two divergent copies.

**Deliberately still deferred:**

- **Real cross-instance discovery, identity, and trust.** Every existing multi-device concept in
  this workspace (`hyperion-federation`, `hyperion-device`) models *one user's own devices* inside
  one process — there is no concept anywhere of a *different* user/instance as a peer yet. This is
  a real, separate identity model, not a small extension of what exists. `/mcp-call`/`/a2a-call`
  work only because the caller already knows the exact host/port to name.
- **mDNS/DNS-SD advertise + discover.** The natural next slice for the gap above's *discovery*
  half (not identity/trust): `/mcp-server`/`/a2a-server` publishing a real
  `_hyperion-mcp._tcp.local.`/`_hyperion-a2a._tcp.local.` service record on the real port they
  bound, and a way to browse for the same service types on the LAN — closing
  `hyperion-device`'s own already-named "real discovery protocols (mDNS/BLE/Matter/cloud-relay)"
  gap at the same time. Not started this pass.
- **The rest of each real spec.** MCP: resources, prompts, notifications, the SSE-streaming half
  of "Streamable HTTP," stdio transport. A2A: `GetTask`/`ListTasks`/streaming/push notifications
  (no real task store exists here — every dispatch completes synchronously before `SendMessage`
  returns, so there's nothing to poll).
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
