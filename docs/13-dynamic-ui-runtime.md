# Dynamic UI Runtime

## 1. Purpose

The Dynamic UI Runtime is the L6 Experience Layer subsystem (see
[02 — Core Architecture](02-core-architecture.md#1-layered-system-view)) responsible for turning an
**Intent** plus a resolved set of **Capabilities** into a concrete, on-screen **Workspace** — a UI
surface "synthesized for the duration of a goal... composed of Capabilities and bound to the
Semantic Objects relevant to the active Intent," per the definition in
[02 — Core Architecture](02-core-architecture.md#2-shared-vocabulary). Nothing in Hyperion ships a
hand-authored screen for "the spreadsheet app" or "the photo editor." Instead, the runtime compiles
a declarative **Workspace UI Graph** — panels bound to Capabilities and Semantic Objects — and
renders it. This document defines that compilation pipeline, the rendering/composition engine, how
[Adaptive Complexity](01-vision-and-philosophy.md#6-adaptive-complexity) changes what gets rendered
for an identical Intent, the Workspace lifecycle from generation through archival or pinning, and
the caching strategy that makes sub-second generation (per
[01 — Vision & Philosophy §10](01-vision-and-philosophy.md#10-success-criteria) and
[36 — Performance Benchmarks](36-performance-benchmarks.md)) achievable.

## 2. Motivation

[01 — Vision & Philosophy §8](01-vision-and-philosophy.md#8-visual-interfaces-still-matter) states
the requirement directly: conversation never replaces visual interfaces, it *generates* them. "I
need a spreadsheet" produces a spreadsheet workspace; "I need to edit photos" produces an
image-editing workspace; "I need to code" produces an IDE; "Prepare for my exam" produces a
workspace containing notes, flashcards, a browser, a calendar, practice questions, a timer, PDFs,
and an AI tutor, assembled together because the task needs them together, not because a single
application bundled them at build time. When the task ends, the workspace is torn down "so nothing
lingers to clutter the next session unless the user chooses to keep it."

This has two direct consequences for architecture that distinguish the runtime from a conventional
UI toolkit:

1. **There is no fixed set of screens.** Every Workspace is compiled at goal-time from declarative
   descriptions supplied by Capabilities (see [11 — Agent Runtime](11-agent-runtime.md) and
   [24 — Plugin Framework](24-plugin-framework.md)), not selected from a menu of pre-built windows.
2. **Generation speed is a first-class correctness property.** If synthesizing a workspace takes
   seconds, conversation stops feeling like the primary interface and starts feeling like a loading
   screen in front of the "real" UI — a direct violation of the
   [Golden Rule](01-vision-and-philosophy.md#2-the-golden-rule).

## 3. Architecture

The runtime sits entirely in L6 but consumes outputs from L4 and L5 on every compile:

```
   Intent Graph (05)      Capability Set (11/12)     Context Bundle (06/07)
        │                        │                          │
        └───────────┬────────────┴─────────────┬────────────┘
                     ▼                          
          ┌───────────────────────┐   compiled-template cache (keyed by
          │  Workspace Compiler   │◄──structural shape, see §5.4)
          │   (Layout Synthesis)  │
          └───────────┬───────────┘
                       ▼
          ┌───────────────────────┐
          │  Workspace UI Graph   │   panels + bindings + layout constraints
          └───────────┬───────────┘
                       ▼
          ┌───────────────────────────────┐
          │ Adaptive Complexity Filter     │──► reads procedural memory (08)
          │ (beginner / pro / dev density) │
          └───────────┬───────────────────┘
                       ▼
          ┌───────────────────────┐
          │  Component Resolver   │   component-library lookup + version pin
          └───────────┬───────────┘
                       ▼
          ┌───────────────────────┐
          │ Responsive Layout      │  constraint solve → concrete grid per
          │ Engine                 │  breakpoint/device (20)
          └───────────┬───────────┘
                       ▼
          ┌───────────────────────┐
          │ Renderer / Compositor  │──► first paint (sandboxed per 15)
          └───────────┬───────────┘
                       ▼
                 Live Workspace
                       │
     Event Bus (31) ───┤ agent results, Semantic Object mutations
                       ▼
          ┌───────────────────────┐
          │ Incremental Re-render  │  (panel-scoped diff, not whole-graph)
          │ (diff & patch)          │
          └───────────┬───────────┘
                       ▼
     pin ────────┐   discard ────────┐   idle-timeout / task-complete ──┐
                  ▼                   ▼                                  ▼
         durable Semantic      torn down,               archived snapshot
         Object (02 §2)        resources freed          (recoverable, 33)
```

The compiler never talks to a device directly and the renderer never talks to an Intent directly —
every stage consumes only the output of the stage before it, which is what allows the compiled
template cache (§5.4) to sit transparently in the middle of the pipeline without either side
knowing it was a cache hit.

## 4. Data Structures

| Structure | Fields | Notes |
|---|---|---|
| `WorkspaceIntentKey` | `intent_shape_hash, capability_set_sig, complexity_tier, device_profile_id` | Cache key; hashes *structural shape*, not literal content, so two different exam subjects reuse one template. |
| `Panel` | `panel_id, capability_ref, region_affinity, min_size, priority, bindings[], accessibility_tree_ref, render_state` | One per Capability surfaced in the Workspace. `accessibility_tree_ref` points into the parallel tree defined in [14 — Accessibility](14-accessibility.md). |
| `Binding` | `panel_id, target (object_id \| live_query), mode {read, read-write, stream}` | Connects a Panel to Semantic Object(s) via the [Knowledge Graph](09-knowledge-graph.md). |
| `WorkspaceGraph` | `graph_id, intent_id, panels[], layout_edges[], lifecycle_state, created_at, ttl` | `lifecycle_state ∈ {generating, live, archived, pinned, discarded}`. |
| `CompiledLayoutTemplate` | `template_id, cache_key, panel_topology, region_assignments, compiled_at, hit_count, lint_result` | Cached artifact; `lint_result` is the accessibility gate outcome from [14 — Accessibility](14-accessibility.md). |
| `LiveUpdateEvent` | `workspace_id, panel_id, event_type {result_ready, progress, error}, payload_ref` | Consumed from [31 — Event System](31-event-system.md). |

## 5. Algorithms

**5.1 Capability-to-panel mapping.** For each Capability in the resolved set (produced by
[05 — Intent Engine](05-intent-engine.md) planning and assigned to Agents by
[12 — Multi-Agent Coordination](12-multi-agent-coordination.md)), the compiler reads that
Capability's declared UI contract (panel type, default region affinity, expected data shape — see
§6) and instantiates a `Panel`, binding it to the Semantic Objects the Context Bundle marked
relevant (see [07 — Context Propagation](07-context-propagation.md)).

**5.2 Responsive layout synthesis.** Panel placement is treated as a constraint-satisfaction
problem over a responsive grid: region affinity, size priority, and reading/focus order are
constraints; the solver (a bounded, incremental variant of a flex/cassowary-style solver) produces
concrete grid coordinates per breakpoint. Because the *topology* is cached (§5.4), this solve
usually runs against an already-known-good constraint set and only needs to re-resolve concrete
pixel values for the current device.

**5.3 Adaptive Complexity density selection.** The same Intent and Capability set can compile to
structurally different graphs. The filter reads the same accumulated procedural/episodic signal
[01 §6](01-vision-and-philosophy.md#6-adaptive-complexity) describes — it is *not* a separate
subsystem, it is a read against [06 — Context Engine](06-context-engine.md) and
[08 — Memory Engine](08-memory-engine.md) — and selects a panel *variant* (for example
`spreadsheet.formula_bar.basic` vs. `spreadsheet.formula_bar.advanced`, or whether a "Kernel
Diagnostics" panel is offered at all). The selection is always transparent and reversible per
[01 §6](01-vision-and-philosophy.md#6-adaptive-complexity) and explainable per
[18 — Explainability & Trust](18-explainability-and-trust.md).

**5.4 Cache key derivation and template reuse.** The cache key hashes the *structural shape* of the
request (Intent type, Capability-set signature, complexity tier, device profile) rather than
literal Semantic Object content. This is the single biggest lever on the sub-second target (§12):
a second "prepare for my exam" Workspace for a different subject is a cache hit on topology and
only pays the cost of re-binding data.

**5.5 Incremental re-render.** The live Workspace subscribes to [31 — Event System](31-event-system.md).
On each `LiveUpdateEvent`, only the affected Panel's binding is diffed and patched — never the whole
graph — and updates within a frame budget are coalesced to avoid visible thrash when an Agent
streams many small results.

**5.6 Lifecycle transition.** Idle-timeout or explicit close discards the graph and frees resources.
Explicit **pin** promotes the `WorkspaceGraph` plus its bound Semantic Objects into a durable
Semantic Object per [02 — Core Architecture §2](02-core-architecture.md#2-shared-vocabulary)
("Workspace" definition). Archival retains a recoverable snapshot for a bounded retention window via
[33 — Rollback & Recovery](33-rollback-recovery.md) rather than deleting outright, so an
accidentally-closed Workspace is never unrecoverable.

## 6. Interfaces / APIs

```
WorkspaceCompiler.compile(intent, capabilitySet, contextBundle, complexityTier) -> WorkspaceGraph
WorkspaceRenderer.mount(graph: WorkspaceGraph) -> RenderedWorkspace
Panel.bind(objectRefs: SemanticObjectRef[]) -> BindingResult
Workspace.pin()      -> SemanticObjectId      # durable per 02 §2
Workspace.archive()  -> SnapshotId            # recoverable per 33
Workspace.discard()  -> void
EventBus.subscribe(workspace_id, handler)     # 31 — Event System
```

Every Capability that wants to be renderable must declare a **Capability UI Contract** (enforced by
tooling in [24 — Plugin Framework](24-plugin-framework.md) and [25 — SDK](25-sdk.md)): a
`panel_template`, `region_affinity`, `min_size`, `data_shape`, and — required, not optional —
`accessibility_hints` consumed by [14 — Accessibility](14-accessibility.md). A Capability with no
UI contract is still invokable by an Agent; it simply never appears as a panel and is represented,
if at all, through the conversational shell.

## 7. Pseudocode

```python
def compile_workspace(intent, capability_set, context_bundle, complexity_tier, device_profile):
    key = WorkspaceIntentKey(
        intent_shape_hash=structural_hash(intent),
        capability_set_sig=signature(capability_set),
        complexity_tier=complexity_tier,
        device_profile_id=device_profile.id,
    )

    template = template_cache.get(key)
    if template is None or template.lint_result.failed:
        panels = []
        for cap in capability_set:
            contract = cap.ui_contract()               # §6
            variant = select_variant(contract, complexity_tier)   # §5.3
            panels.append(Panel(
                capability_ref=cap.ref,
                region_affinity=contract.region_affinity,
                min_size=contract.min_size,
                priority=contract.priority,
                render_state="pending",
            ))
        topology = solve_layout_constraints(panels)     # §5.2
        template = CompiledLayoutTemplate(
            cache_key=key,
            panel_topology=topology,
            lint_result=accessibility_linter.lint(topology),  # 14 — gate
        )
        if template.lint_result.failed:
            template = fallback_generic_template(panels)
        template_cache.put(key, template)

    graph = instantiate_graph(template, intent.id)
    for panel in graph.panels:
        objects = context_bundle.relevant_objects_for(panel.capability_ref)
        panel.bind(objects)                              # §5.1

    graph.lifecycle_state = "generating"
    rendered = renderer.mount(graph)                      # first paint
    graph.lifecycle_state = "live"
    event_bus.subscribe(graph.id, on_live_update)          # §5.5
    return graph


def on_live_update(event):
    panel = graph_registry[event.workspace_id].panel(event.panel_id)
    patch = diff(panel.render_state, event.payload_ref)
    renderer.patch(panel, patch)                          # incremental only
```

## Worked Example

Utterance: **"Help me prepare for tomorrow's exam."** [05 — Intent Engine](05-intent-engine.md)
decomposes this into sub-intents (review notes, generate practice questions, schedule study
blocks, retrieve source PDFs) and [12 — Multi-Agent Coordination](12-multi-agent-coordination.md)
assigns them to a Study Agent, a Scheduling Agent, and a Tutor Agent, each invoking Capabilities
(`notes.summarize`, `flashcard.generate`, `calendar.read`, `document.render`,
`tutor.converse`) exactly as traced in
[02 — Core Architecture §3](02-core-architecture.md#3-how-a-request-flows-through-the-layers).

The compiler produces a `WorkspaceGraph` with eight panels:

| Panel | Capability | Bound Semantic Object(s) | Region |
|---|---|---|---|
| Notes | `notes.summarize` | Course notes (existing objects) | left column, top |
| Flashcards | `flashcard.generate` | New flashcard objects (streamed in live) | left column, bottom |
| Browser | `web.research` | Ephemeral browsing session | center, tabbed |
| Calendar | `calendar.read` | "Exam — 9:00 AM" event object | top bar |
| Practice Questions | `quiz.generate` | Generated question-set object | center, primary |
| Timer | `timer.session` | Session-local state (not persisted) | top bar, small |
| PDFs | `document.render` | Syllabus + textbook chapter objects | right column |
| AI Tutor | `tutor.converse` | Conversation thread object | right column, docked |

At the beginner complexity tier, the Practice Questions panel shows one question at a time with
large controls; at the pro tier the same Intent and Capability set compile a denser variant showing
a scrollable question bank with keyboard shortcuts — the *topology* (eight panels, same regions) is
identical and comes from the same cached template; only the panel *variant* selected in §5.3
differs. As the Study Agent finishes generating flashcards, `LiveUpdateEvent`s patch the Flashcards
panel incrementally — the user sees the first three cards while the Agent is still producing the
tenth. When the exam is over, the user either lets the Workspace idle-time out (discarded), or says
"keep this for finals" — which pins it, converting the graph and its bound notes, flashcards, and
question sets into a durable Semantic Object per [02 §2](02-core-architecture.md#2-shared-vocabulary).

## 8. Security Considerations

Every Panel's data binding is scoped by the same capability token that authorized the underlying
Capability invocation (see [02 §5](02-core-architecture.md#5-capability-security-as-the-unifying-security-model)
and [15 — Security Architecture](15-security-architecture.md)) — a Panel can never bind to a
Semantic Object its Capability was not separately granted access to, even if the object is visible
elsewhere in the same Workspace. Third-party Capability-authored panel components render inside an
isolated compositor surface (a Trust Boundary crossing per
[02 §2](02-core-architecture.md#2-shared-vocabulary), enforced by the sandboxing primitives in
[03 — Kernel Architecture](03-kernel-architecture.md)), so a malicious or buggy component cannot
read another panel's bound data or escalate into the compositor process. Model-generated UI content
(labels, summaries) is treated as untrusted input and sanitized before it reaches the renderer, to
close off both classic injection and prompt-injection-reflected-into-UI vectors. The compiled
template cache key includes a signature of the requesting principal's permission set specifically
so a template compiled under a higher-privilege session can never be replayed for a lower-privilege
one.

## 9. Failure Modes

- A Capability invocation times out or is unavailable, leaving its Panel stuck in `pending`.
- The layout solver is handed a contradictory constraint set (e.g., two panels both claiming the
  only "primary" region) and would otherwise emit a cyclic or invalid graph.
- The Component Resolver cannot find a renderer for a declared panel type after a version skew
  introduced by [32 — Update System](32-update-system.md).
- A runaway Agent floods the Event Bus, overwhelming the incremental re-render path.
- A cached template is replayed against a Capability whose contract changed since compilation.
- The Adaptive Complexity filter misjudges tier and renders the wrong density for the user.

## 10. Recovery Mechanisms

Consistent with Design Invariant 5, "degrade, never fail closed"
([02 §4](02-core-architecture.md#4-design-invariants)): a stuck Panel shows a placeholder with an
explanation and a retry affordance rather than blocking the whole Workspace; a missing renderer
falls back to a generic "raw data viewer" component; the graph validator rejects cyclic layouts at
compile time and falls back to the last known-good cached template; the Event Bus applies
backpressure and a per-workspace circuit breaker under load; cached templates are invalidated on
Capability contract version bumps tracked by [32 — Update System](32-update-system.md); complexity
tier is always user-overridable ("show me everything," "simplify this") per
[01 §6](01-vision-and-philosophy.md#6-adaptive-complexity); and any bad live-update can be rolled
back to a prior Workspace checkpoint via [33 — Rollback & Recovery](33-rollback-recovery.md).

## 11. Performance Analysis

The sub-second generation target from
[01 §10](01-vision-and-philosophy.md#10-success-criteria) and
[36 — Performance Benchmarks](36-performance-benchmarks.md) is achievable only because most of the
pipeline in §3 is cache-amortized, not because any single stage is intrinsically fast. Indicative
budget for a cache-hit compile:

| Stage | Budget |
|---|---|
| Template cache lookup | ~10 ms |
| Capability/object binding resolution (parallelized) | 50–150 ms |
| Component resolution (precompiled local library) | ~20 ms |
| Responsive layout solve (topology already known) | 20–50 ms |
| First paint | < 300 ms |
| Full hydration (long-running Agent results streaming in) | unbounded, but never blocks first paint |

Idle-time precompilation of the top-N templates for a user's common Intents (informed by
[08 — Memory Engine](08-memory-engine.md) usage history) keeps the common case a cache hit.
Incremental rendering means a long-running Research Agent never blocks first paint — panels mount
in a pending state and hydrate as [31 — Event System](31-event-system.md) delivers results, which
is what lets "sub-second generation" and "the exam workspace has a browser doing live web research"
coexist without contradiction.

## 12. Trade-offs

Declarative, compiled UI trades pixel-perfect, hand-tuned craft for consistency, speed, and the
ability to synthesize interfaces that were never designed together in advance — acceptable under
the [Golden Rule](01-vision-and-philosophy.md#2-the-golden-rule) because it is what makes
goal-driven generation possible at all; Capability authors can still supply richer panel templates
via [25 — SDK](25-sdk.md) without breaking the underlying binding model. Aggressive template
caching improves latency but introduces a staleness/version-skew risk, mitigated by signature-based
invalidation (§8, §10). Ephemerality-by-default (per
[01 §8](01-vision-and-philosophy.md#8-visual-interfaces-still-matter)) trades persistence for a
clutter-free default, with **pin** as the explicit escape hatch. Adaptive Complexity trades
predictability for personalization, which is why it must remain transparent and reversible per
[01 §6](01-vision-and-philosophy.md#6-adaptive-complexity) rather than a silent heuristic.

## 13. Testing Strategy

Golden-layout snapshot tests exist per canonical Intent (spreadsheet, IDE, exam-prep) across all
three complexity tiers; property-based fuzz testing generates random Capability-set combinations
and asserts the compiler never emits an invalid or cyclic graph; every `CompiledLayoutTemplate` must
pass the accessibility linter defined in [14 — Accessibility](14-accessibility.md) before entering
the cache, gating template promotion the same way a build gate would; load/soak tests simulate
Event Bus storms to validate backpressure; visual regression runs across component-library version
bumps tied to [32 — Update System](32-update-system.md) releases. All of the above feed the shared
harness and CI gating defined in [35 — Testing Strategy](35-testing-strategy.md).

---
*Next: [14 — Accessibility](14-accessibility.md).*
