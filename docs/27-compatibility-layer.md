# Compatibility Layer

## Purpose

This document specifies how Hyperion runs software that is not, and will never be, a
[Capability](02-core-architecture.md#capability): existing Windows software, Linux software,
Android applications, web applications, CLI tools, virtual machines, and containers. It covers how
such software is hosted inside a Trust Boundary using the container/VM runtime introduced in
[03 — Kernel Architecture](03-kernel-architecture.md#virtualization-and-container-runtime), how a
translation shim maps its filesystem calls onto the
[Semantic Filesystem](10-semantic-filesystem.md)'s POSIX-compatibility view, how it is presented to
the rest of the system as a [Workspace](02-core-architecture.md#workspace), the security boundary
around code that cannot itself participate in capability security, and where compatibility
intentionally and permanently stops. It does not redefine the container/VM mechanism itself (owned
by [03](03-kernel-architecture.md)), the manifest/permission model for actual Capabilities (owned
by [24 — Plugin Framework](24-plugin-framework.md)), or the Semantic Filesystem's storage model
(owned by [10](10-semantic-filesystem.md) and [28 — Storage Engine](28-storage-engine.md)).

## Motivation

[01 §1](01-vision-and-philosophy.md#1-what-hyperion-is) is explicit that Hyperion is a ground-up
rethinking of what an OS manages, not Linux or Windows with a chatbot bolted on — but the source
requirement for this layer is equally explicit: support existing software *whenever practical*,
without compromising that architecture. These two constraints are in real tension, and this
document resolves it by refusing to let legacy software touch the architecture at all rather than
diluting the architecture to accommodate it. A Windows binary cannot request a capability token; it
cannot decompose into an Intent; it cannot be trusted to respect
[02 §4](02-core-architecture.md#4-design-invariants)'s "no silent authority" invariant, because it
was never written with that invariant in mind. Hyperion's answer is to treat all such software as
**fully untrusted by construction**, host it inside the deepest Trust Boundary its risk profile
warrants, and mediate every crossing of that boundary explicitly — so legacy support is additive
(a capability-scoped sandbox with a foreign OS inside it) rather than corrosive (a hole punched in
the capability model to let old code through).

## Architecture

A legacy application is hosted by a **Compatibility Host**: a container or VM monitor — themselves
unprivileged, capability-scoped servers per
[03 — Kernel Architecture](03-kernel-architecture.md#virtualization-and-container-runtime) — paired
with a **translation shim** that mediates the three things the rest of Hyperion needs from any
running software: filesystem access, screen presentation, and (best-effort) data egress into the
[Knowledge Graph](09-knowledge-graph.md).

```
┌─────────────────────────────────────────────────────────────────────────┐
│  REST OF HYPERION                                                       │
│  ┌───────────────────────┐   ┌─────────────────────────────────────┐   │
│  │ Workspace (13-dynamic- │   │ Knowledge Graph (09) — receives only │   │
│  │ ui-runtime.md)         │◀──┤ promoted Semantic Objects, never raw │   │
│  │  window surface only   │   │ guest state (§Algorithms)            │   │
│  └───────────┬───────────┘   └────────────────▲────────────────────┘   │
└──────────────┼───────────────────────────────┼──────────────────────────┘
               │ captured framebuffer /          │ promoted artifacts
               │ compositor surface              │ (opt-in, §Algorithms)
┌──────────────▼─────────────────────────────────┴──────────────────────┐
│                   TRANSLATION SHIM  (Compatibility Host edge)          │
│  ┌────────────────┐ ┌───────────────────┐ ┌─────────────────────────┐│
│  │ Path translation│ │ Window/UI capture │ │ Artifact promotion scan ││
│  │ → 10-semantic-  │ │ → Workspace binding│ │ → draft Semantic Object││
│  │   filesystem.md │ │                   │ │   (never silent)        ││
│  └────────────────┘ └───────────────────┘ └─────────────────────────┘│
│  All crossings mediated here — the guest itself holds *zero*          │
│  capability tokens; only the shim/host does (§Security Considerations)│
├─────────────────────────────────────────────────────────────────────────┤
│  GUEST                                                                  │
│  ┌───────────────────────────┐   ┌───────────────────────────────────┐│
│  │ Depth 2 — CONTAINER        │   │ Depth 3 — VM                      ││
│  │ shared Linux-derived kernel│   │ foreign guest kernel, hardware-    ││
│  │ ABI, namespaced process(es)│   │ virtualized device model           ││
│  │  Linux CLI tools · Android │   │  Windows applications · foreign-   ││
│  │  apps (translated surface) │   │  kernel or fully untrusted VMs     ││
│  └───────────────────────────┘   └───────────────────────────────────┘│
├─────────────────────────────────────────────────────────────────────────┤
│      Container/VM Monitor — unprivileged server holding hardware-       │
│      virtualization capabilities minted by the Capability Monitor       │
│      (03-kernel-architecture.md §Virtualization and Container Runtime)  │
└─────────────────────────────────────────────────────────────────────────┘
```

Web applications and CLI tools are the shallow end of this same spectrum: a web app typically runs
at **depth 0/1** (an in-process or process-isolated browser engine instance) rather than depth 2/3,
since its own runtime is already largely capability-securable (origin sandboxing); a CLI tool runs
at depth 1 by default and is promoted to depth 2 only if it requests filesystem or network access
beyond its own working set — the same "attenuate and isolate further" admission decision described
in [03 — Kernel Architecture](03-kernel-architecture.md#sandboxing-as-one-spectrum), not a special
case for compatibility.

**Depth selection** follows the table already established in
[03 — Kernel Architecture](03-kernel-architecture.md#sandboxing-as-one-spectrum): Android
applications default to depth 2 (shared Linux-derived kernel ABI, translated Capability surface for
Android's own permission model); Windows applications default to depth 3 (foreign kernel, full
hardware virtualization); Linux software is admitted at whichever depth its own declared trust
level warrants, from depth 1 (a recompiled, cooperative CLI tool) up to depth 2 (an unmodified
binary needing a full namespace). This is an admission-time policy decision, not a hardcoded rule
per platform, exactly as [03](03-kernel-architecture.md) specifies for Capabilities generally.

## Data Structures

```rust
/// Declares which legacy target a Compatibility Host is instantiated for and
/// what minimum Trust Boundary depth (03-kernel-architecture.md) it requires.
struct CompatibilityProfile {
    target: LegacyTarget,           // Windows | Linux | Android | Web | Cli | Vm | Container
    min_depth: TrustDepth,          // 0..3, admission floor per target class
    network_default: NetworkPolicy, // Deny | LoopbackOnly | Allow(scope)
    filesystem_roots: Vec<SemanticRoot>, // declared mount points, never full host FS
    accessibility_bridge: Platform | PixelFallback | None, // §Algorithms "Accessibility bridging";
                                     // drives 13's disclosure chrome and 14's linter matrix
}

/// One running instance of a legacy application inside its Compatibility Host.
struct CompatSession {
    session_id: SessionId,
    boundary: TrustBoundaryId,      // minted by the Capability Monitor, 03-*.md
    profile: CompatibilityProfile,
    workspace: Option<WorkspaceId>, // bound Workspace, 13-dynamic-ui-runtime.md
    grants: Vec<CapabilityToken>,   // explicit, narrow — never ambient (§Security)
}

/// Maps a guest-visible path to the Semantic Filesystem's POSIX-compat view.
/// The guest only ever sees paths under `guest_root`; nothing else resolves.
struct ShimPathMapping {
    guest_root: String,             // e.g. "C:\Users\Guest\Documents"
    semantic_root: SemanticObjectId, // a folder-view root, per 10-semantic-filesystem.md
    case_sensitivity: CaseMode,      // guest-OS-appropriate, resolved before lookup
}

/// A file the shim has observed being written/exported inside the guest's
/// declared "documents" area, pending user or policy decision on promotion.
struct IngestedArtifact {
    guest_path: String,
    sniffed_type: SemanticType,      // best-effort MIME/type sniff
    promotion_state: "pending" | "promoted" | "ignored",
    draft_object: Option<SemanticObjectDraft>,
}
```

## Algorithms

**Path translation.** Every filesystem call the guest issues is intercepted at the shim and
resolved against a `ShimPathMapping` before it is allowed to reach the Semantic Filesystem's
POSIX-compat view (per [10 — Semantic Filesystem](10-semantic-filesystem.md)); a path outside every
declared `guest_root` is rejected at the shim, not passed through and rejected deeper in the stack,
so a legacy app's filesystem view is a strict, enumerable subset of the real Semantic Object graph,
never the whole thing.

**Artifact promotion.** The shim watches writes and exports inside a guest's declared document
roots, sniffs the resulting file's type, and creates an `IngestedArtifact` in `pending` state — it
never writes directly into the [Knowledge Graph](09-knowledge-graph.md). Promotion to a real
[Semantic Object](02-core-architecture.md#semantic-object) happens only on explicit user
confirmation or a standing user policy ("always promote exports from this app"), consistent with
[02 §4](02-core-architecture.md#4-design-invariants)'s "no silent authority" — a legacy app's
output becoming a first-class, Knowledge-Graph-linked object is itself an authority crossing and is
treated as one.

**Window-to-Workspace binding.** The Compatibility Host captures the guest's framebuffer or
compositor surface (container: a namespaced Wayland/X11-equivalent surface; VM: a virtual GPU
output) and wraps it as the sole content of an otherwise ordinary
[Workspace](02-core-architecture.md#workspace), per
[13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md); to the rest of Hyperion this Workspace is
indistinguishable in kind from a natively generated one — it can be pinned, torn down, or
snapshotted identically — but its *content* is opaque pixels rather than declaratively composed
Capabilities, which is the architectural line this document draws (see §Trade-offs).

**Accessibility bridging (bounded exception to [02 §4](02-core-architecture.md#4-design-invariants)'s
Invariant 6).** Opaque pixel content has no `accessible_role`/`label_template`/`keyboard_operations`
for [14 — Accessibility](14-accessibility.md)'s compiler pass to derive an `AccessibilityTree`
from, because there is no Capability contract behind it — Invariant 6 cannot be satisfied the same
way here as for a natively generated Workspace, and this document says so explicitly rather than
leaving it unaddressed. The Compatibility Host mitigates in two tiers, chosen automatically per
guest and surfaced to the user, never silently:
1. **Platform accessibility bridge (preferred).** Where the guest OS exposes its own native
   accessibility API (Windows UI Automation, Android `AccessibilityService`, X11 AT-SPI), the
   Compatibility Host runs a bridge process inside the same Trust Boundary that subscribes to that
   API and translates its role/name/action tree into a best-effort `AccessibilityTree` (14 §4),
   letting a screen reader, switch-scan device, or voice-control grammar operate against the guest
   nearly as if it were native. This is necessarily lower-fidelity than a Capability-derived tree —
   the bridge cannot correct a guest app's own accessibility bugs — but it is real, structural
   accessibility, not a pixel-level approximation.
2. **Pixel-level fallback**, used only when no platform bridge is available or the guest exposes no
   accessibility API at all: system-level magnification, high-contrast/color-filter overlays, and
   cursor/focus-ring highlighting apply to the raw framebuffer regardless of its content, and an
   OCR-based text extraction pass gives a screen reader a best-effort, explicitly-lower-fidelity
   textual approximation of on-screen content. This tier does not claim parity with a real
   accessibility tree and is not treated as one by the linter (below).

Every Compatibility Workspace's `CompatibilityProfile` records which tier is active
(`accessibility_bridge: Platform | PixelFallback | None`), and [13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md)
renders a persistent, dismissable disclosure on the Workspace chrome — "Limited accessibility:
legacy application" — whenever the tier is not `Platform`, so a user relying on assistive
technology is told this Workspace is the bounded exception per [01 §5](01-vision-and-philosophy.md#5-universal-usability-highest-priority),
not silently left behind. [14 — Accessibility](14-accessibility.md)'s linter (§13) records the
active tier per Compatibility Workspace in its conformance matrix rather than skipping compat
Workspaces from testing entirely — a `None`-tier session with no user-facing disclosure is treated
as a linter failure, the same severity as a missing `accessible_role` on a native Capability.

**Admission and escalation.** A `CompatSession`'s initial depth is chosen from
`CompatibilityProfile.min_depth`; if the guest attempts an operation its current depth cannot
safely mediate (e.g., a Linux CLI tool suddenly requesting raw device access), the shim does not
silently grant it — the session is either denied and logged, or the user is prompted to explicitly
re-launch at a deeper boundary, mirroring the kernel's own promotion decision in
[03 — Kernel Architecture](03-kernel-architecture.md#algorithms).

## Interfaces / APIs

```
compat_launch(profile: CompatibilityProfile, target: AppIdentifier) -> CompatSession
compat_bind_workspace(session: SessionId) -> WorkspaceId              // 13-dynamic-ui-runtime.md
compat_grant(session: SessionId, cap: CapabilityRequest) -> Result<CapabilityToken, Denied>
compat_capture_artifact(session: SessionId, path: String) -> Option<IngestedArtifact>
compat_promote_artifact(artifact: IngestedArtifact, policy: PromotionPolicy) -> Result<SemanticObjectId, Denied>
compat_terminate(session: SessionId) -> Unit
```

`compat_grant` is the only path by which a running legacy session gains any capability beyond its
launch-time defaults, and every grant is scoped, logged, and revocable exactly like any other
capability derivation in [03 — Kernel Architecture](03-kernel-architecture.md#algorithms) — there
is no separate "legacy app permission" model, only the same capability model applied at a coarser,
host-mediated grain.

## Pseudocode

```rust
// Shim-side syscall interception for filesystem calls — the mediation point
// that keeps a fully untrusted guest from ever touching more than its
// declared roots, regardless of what the guest OS's own permission model says.
fn shim_open(session: &CompatSession, guest_path: &str, mode: OpenMode) -> Result<FileHandle, Fault> {
    let mapping = session.profile.filesystem_roots.iter()
        .find(|root| guest_path.starts_with(&root.guest_root))
        .ok_or(Fault::PathOutsideDeclaredRoots)?;   // default deny, not default allow

    let resolved = semantic_filesystem::resolve_posix_view(
        mapping.semantic_root, guest_path, mapping.case_sensitivity,
    )?;                                              // 10-semantic-filesystem.md

    if mode.is_write() && !session.grants.iter().any(|g| g.permits_write(resolved.object_id)) {
        return Err(Fault::WriteNotGranted);
    }

    let handle = semantic_filesystem::open(resolved.object_id, mode)?;

    // Writes inside a declared "documents" root are candidates for promotion,
    // but promotion itself never happens here — it is a separate, explicit step.
    if mode.is_write() && mapping.is_document_root {
        artifact_watcher::mark_pending(session.session_id, guest_path);
    }
    Ok(handle)
}

// Promotion is always a separate, explicit decision — never folded into the
// write path above, so a legacy app cannot cause a Knowledge Graph write
// merely by saving a file.
fn promote_artifact(artifact: IngestedArtifact, policy: PromotionPolicy) -> Result<SemanticObjectId, Fault> {
    match policy {
        PromotionPolicy::AskEveryTime => {
            let decision = ui::prompt_user(PromotionPrompt::from(&artifact));  // 13-dynamic-ui-runtime.md
            if !decision.approved { return Err(Fault::PromotionDeclined); }
        }
        PromotionPolicy::StandingRule(rule) if rule.matches(&artifact) => { /* proceed */ }
        _ => return Err(Fault::PromotionDeclined),
    }
    let draft = classify_and_draft(&artifact)?;      // best-effort type + metadata inference
    knowledge_graph::write(draft)                     // 09-knowledge-graph.md, via 26-apis.md
}
```

## Security Considerations

Legacy code is treated as **fully untrusted by construction**, not merely "less trusted than a
Capability" — it holds zero capability tokens of its own; every token in play belongs to the
Compatibility Host or the shim mediating on its behalf, so a compromised guest inherits nothing
ambient (this is the same confused-deputy prevention described in
[03 — Kernel Architecture](03-kernel-architecture.md#security-considerations), applied at host
granularity). Network egress defaults to deny; a legacy app that needs network access must have it
explicitly granted per `CompatibilityProfile.network_default`, and that grant is visible and
revocable exactly like any Capability's. For a `LegacyTarget::Web` session specifically,
`NetworkPolicy::Allow(scope)` resolves at admission time into a `web.fetch.raw` capability grant
scoped to that same `scope` — see [19 — Networking Stack](19-networking-stack.md#31-relationship-to-the-compatibility-layer)
— so the raw-browsing Capability that document describes is the concrete mechanism behind this
policy enum's `Allow` variant for Web targets, not a second, unrelated network-access path; for
non-Web targets (a Linux CLI tool, an Android app), `Allow(scope)` is enforced directly by the
Compatibility Host's network namespace with no Capability indirection, since there is no browser
semantic layer above the transport for those guests. The guest never sees the real host
filesystem — only the
POSIX-compat view scoped to its declared `filesystem_roots` — so path traversal, symlink tricks, or
assumptions about a shared root are contained at the shim boundary rather than relying on the guest
OS's own (untrusted) permission enforcement. Clipboard and inter-Workspace data flow between a
legacy Workspace and the rest of the system is treated as a Trust Boundary crossing requiring
explicit grant, since it is a realistic exfiltration path between fully untrusted guest code and a
user's real Semantic Objects. A VM/container escape attempt is detected identically to any other
capability-rights violation at the hypervisor's own token boundary, per
[03 — Kernel Architecture](03-kernel-architecture.md#security-considerations), and reported to
[17 — Threat Model](17-threat-model.md) instrumentation — there is no special-cased "legacy escape"
detector, because the enforcement point does not distinguish why a boundary was crossed, only that
it was attempted without a token.

## Failure Modes

- **Path translation ambiguity.** Case-insensitivity assumptions (Windows), symlink or bind-mount
  tricks, or `..`-style traversal attempts against the shim's path mapping.
- **Guest crash or hang.** A misbehaving legacy binary hangs the container/VM without crashing it
  cleanly, holding its Trust Boundary's resources indefinitely.
- **Artifact misclassification.** The type-sniffing heuristic promotes a junk or incorrectly typed
  Semantic Object, or fails to recognize a genuinely valuable export.
- **UI capture desync.** The captured framebuffer lags or tears relative to actual guest state,
  especially under resource pressure.
- **Resource exhaustion.** An unbounded legacy workload (a runaway VM process) starves other
  Workspaces sharing the device.

## Recovery Mechanisms

Path traversal and mapping-ambiguity attempts are treated as faults identical to the kernel's
IOMMU-misconfiguration handling in [03 — Kernel Architecture](03-kernel-architecture.md#failure-modes):
denied, logged, and reported, never silently normalized to "probably what the guest meant." A
hung or crashed guest is recovered via the same supervisor-tree "microreboot" pattern
[03 — Kernel Architecture](03-kernel-architecture.md#recovery-mechanisms) uses for drivers — the
Compatibility Host is restarted from its `CompatibilityProfile` with a fresh capability set, and
because the host holds no state the guest itself needs to survive a restart of the *host* (as
opposed to the guest OS instance), a session snapshot (per
[33 — Rollback & Recovery](33-rollback-recovery.md)) lets the user's open windows resume rather
than vanish. Artifact misclassification is always correctable after the fact — promotion is opt-in
and staged, so a misclassified object can be reclassified or deleted by the user without touching
any other part of the [Knowledge Graph](09-knowledge-graph.md), and standing promotion rules can be
revoked the same way any capability grant can be. Resource exhaustion is bounded by the same
resource ledger entry every Trust Boundary gets from
[04 — Scheduler](04-scheduler.md); a runaway guest is throttled or suspended by policy rather than
allowed to degrade the rest of the device, consistent with the scheduler's fairness guarantees for
any sandboxed workload.

## Performance Analysis

Depth-2 containers share the host kernel ABI and are budgeted for near-native overhead — the same
namespace-isolation cost any container workload pays, independent of this being a compatibility
use case. Depth-3 VMs carry the higher, harder floor of hardware virtualization (EPT/NPT page
tables, virtual interrupt injection) and are budgeted accordingly in
[36 — Performance Benchmarks](36-performance-benchmarks.md) as a distinct class from native
Capability execution, not held to the same latency target. Shim path translation adds a small,
constant per-call overhead (one mapping lookup plus one Semantic Filesystem resolve) on top of
whatever the guest's own filesystem call already cost, which is negligible relative to the
virtualization overhead it rides alongside for VM-hosted targets and more noticeable, though still
small, for container-hosted ones. Window capture and Workspace binding are budgeted against
[13 — Dynamic UI Runtime](13-dynamic-ui-runtime.md)'s frame-latency targets specifically for the
"legacy Workspace" case, since a captured framebuffer cannot benefit from the same declarative,
incrementally-rendered composition a native Workspace gets.

## Trade-offs

Running unmodified legacy binaries buys real practicality — a user is not blocked from software
Hyperion does not yet have a native Capability for — at the direct cost of the guarantees
[01 §9](01-vision-and-philosophy.md#9-human-control-is-non-negotiable) requires of everything else
in the system: a legacy app's *internal* actions are not interruptible, undoable, auditable, or
explainable in the way a Capability's are, because they happen inside an opaque guest Hyperion
cannot see into. This document's answer is to draw the guarantee boundary at the Compatibility
Host's edge rather than pretend it extends inside the guest: everything Hyperion can observe and
control about a legacy app (its network egress, its filesystem view, its lifecycle, its promoted
artifacts) gets the full guarantee; what happens inside the guest's own process space does not, and
is never represented to the user as if it did. Container versus VM is a second explicit trade:
containers are cheaper and share more with the host (lower isolation, per
[03 — Kernel Architecture §Sandboxing as One Spectrum](03-kernel-architecture.md#sandboxing-as-one-spectrum)),
VMs are more expensive and isolate more (a genuinely foreign kernel, unable to directly observe host
state at all); Hyperion defaults each legacy target class to the point on that spectrum its typical
risk profile warrants (Android → depth 2, Windows → depth 3) rather than maximizing isolation
universally at a cost every lightweight CLI tool would otherwise have to pay.

**Where compatibility intentionally stops.** A legacy application never receives Agent-level
automation (no [Agent](02-core-architecture.md#agent) drives its internals, since that would
require the same semantic understanding of its state that a Capability provides and a legacy binary
does not) and never receives ambient [Knowledge Graph](09-knowledge-graph.md) integration for data
it holds internally — only artifacts it explicitly exports through the promotion path in
§Algorithms become Semantic Objects. Either of these can be added for a specific application only
through an explicit **bridge**: a real [Capability](02-core-architecture.md#capability), built with
[25 — SDK](25-sdk.md), reviewed through the same marketplace gate as any other Capability per
[15 — Security Architecture](15-security-architecture.md), that mediates a specific, narrow
automation surface (e.g., a UI-automation Capability that drives one legacy app's accessibility
tree) — never a blanket exception granted to the Compatibility Host itself.

## Testing Strategy

A conformance matrix exercises each `LegacyTarget` (Windows, Linux, Android, Web, CLI, VM,
Container) against its default depth, verifying launch, path-translation correctness, Workspace
binding, and clean termination. The shim's translation layer is fuzzed directly with malformed and
adversarial path requests (traversal attempts, case-folding edge cases, symlink loops) independent
of any specific guest OS, since the shim — not the guest — is the trusted component. A dedicated
escape-attempt suite, shared with [03 — Kernel Architecture](03-kernel-architecture.md#testing-strategy)'s
fault-injection testing, verifies that no combination of guest behavior can obtain a capability
token the Compatibility Host did not explicitly grant. Artifact promotion accuracy is tracked as a
precision/recall metric against a labeled corpus of legacy export formats, since both false
promotions (KG pollution) and false negatives (a genuinely valuable export never offered for
promotion) are regressions worth catching independently. Compatibility Hosts destined for
sensitive permission grants (network access beyond loopback, cross-Workspace data flow) pass
through the same review gate [25 — SDK](25-sdk.md) and
[15 — Security Architecture](15-security-architecture.md) define for any Capability, so a
compatibility bridge cannot become a lower-scrutiny path to the same authority.

---
*Next: [28 — Storage Engine](28-storage-engine.md).*
